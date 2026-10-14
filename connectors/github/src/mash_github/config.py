"""Strict TOML configuration; GitHub and Mash tokens stay in the environment."""

import tomllib
from datetime import datetime, timedelta
from enum import StrEnum
from pathlib import Path
from typing import Annotated, Literal

from mash_sdk import Destination
from pydantic import BaseModel, ConfigDict, Field, StrictInt, StrictStr

# GitHub owner and repository names; the pattern also keeps them inert in URLs.
Repository = Annotated[
    StrictStr, Field(pattern=r"^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$")
]


class ItemKind(StrEnum):
    issue = "issue"
    pull_request = "pull-request"


class Local(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    type: Literal["local"]
    database: Path
    executable: StrictStr = "mash"
    config: Path | None = None


class Shared(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    type: Literal["shared"]
    api_url: StrictStr
    group_id: StrictStr
    node_id: StrictStr
    token_env: StrictStr


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
    destinations: tuple[Annotated[Local | Shared, Field(discriminator="type")], ...]

    @property
    def source_id(self) -> str:
        return f"github:{self.account_id}"

    def cutoff(self, now: datetime) -> datetime | None:
        """Creation-time window start; None ingests all history."""
        if self.history_days is None:
            return None
        return now - timedelta(days=self.history_days)

    def targets(self) -> tuple[Destination, ...]:
        return tuple(
            Destination.local(
                database=item.database, executable=item.executable, config=item.config
            )
            if isinstance(item, Local)
            else Destination.shared(
                api_url=item.api_url,
                group_id=item.group_id,
                node_id=item.node_id,
                token_env=item.token_env,
            )
            for item in self.destinations
        )


def load(path: Path) -> Settings:
    with path.open("rb") as stream:
        settings = Settings.model_validate(tomllib.load(stream))
    base = path.resolve().parent

    def absolute(value: Path) -> Path:
        value = value.expanduser()
        return (value if value.is_absolute() else base / value).resolve()

    settings = settings.model_copy(
        update={
            "state_dir": absolute(settings.state_dir),
            "destinations": tuple(
                item.model_copy(
                    update={
                        "database": absolute(item.database),
                        "config": None
                        if item.config is None
                        else absolute(item.config),
                    }
                )
                if isinstance(item, Local)
                else item
                for item in settings.destinations
            ),
        }
    )
    if not settings.destinations:
        raise ValueError("configure at least one destination")
    settings.targets()  # Validate destination identifiers, URLs and token names now.
    return settings
