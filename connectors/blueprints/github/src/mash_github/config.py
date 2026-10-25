"""Strict TOML configuration; GitHub and Mash tokens stay in the environment."""

import tomllib
from datetime import datetime, timedelta
from enum import StrEnum
from pathlib import Path
from typing import Annotated

from mash_sdk import Destination
from mash_sdk.config import DestinationConfig, absolute
from pydantic import BaseModel, ConfigDict, Field, StrictInt, StrictStr

# GitHub owner and repository names; the pattern also keeps them inert in URLs.
Repository = Annotated[
    StrictStr, Field(pattern=r"^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$")
]


class ItemKind(StrEnum):
    issue = "issue"
    pull_request = "pull-request"


class Settings(BaseModel):
    """An empty repository allowlist selects nothing."""

    model_config = ConfigDict(extra="forbid", frozen=True)
    account_id: StrictStr = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$")
    token_env: StrictStr = Field(
        default="GITHUB_TOKEN", pattern=r"^[A-Za-z_][A-Za-z0-9_]*$"
    )
    state_dir: Path
    repositories: tuple[Repository, ...]
    item_kinds: tuple[ItemKind, ...] = (ItemKind.issue, ItemKind.pull_request)
    history_days: StrictInt | None = Field(default=None, gt=0, le=365000)
    poll_seconds: float = Field(default=60, gt=0, allow_inf_nan=False)
    destinations: tuple[DestinationConfig, ...]

    @property
    def source_id(self) -> str:
        return f"github:{self.account_id}"

    def cutoff(self, now: datetime) -> datetime | None:
        """Creation-time window start; None ingests all history."""
        if self.history_days is None:
            return None
        return now - timedelta(days=self.history_days)

    def targets(self) -> tuple[Destination, ...]:
        return tuple(item.destination() for item in self.destinations)


def load(path: Path) -> Settings:
    with path.open("rb") as stream:
        settings = Settings.model_validate(tomllib.load(stream))
    base = path.resolve().parent

    settings = settings.model_copy(
        update={
            "state_dir": absolute(settings.state_dir, base),
            "destinations": tuple(
                item.relative_to(base) for item in settings.destinations
            ),
        }
    )
    if not settings.destinations:
        raise ValueError("configure at least one destination")
    settings.targets()  # Validate destination identifiers, URLs and token names now.
    return settings
