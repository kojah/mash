"""Strict TOML configuration; Slack and Mash tokens stay in the environment."""

import tomllib
from datetime import datetime, timedelta
from pathlib import Path
from typing import Annotated

from mash_sdk import Destination
from mash_sdk.config import DestinationConfig, absolute
from pydantic import BaseModel, ConfigDict, Field, StrictInt, StrictStr

# Public (C) and private or group (G) channels and direct messages (D).
ChannelId = Annotated[StrictStr, Field(pattern=r"^[CGD][A-Z0-9]{2,}$")]


class Settings(BaseModel):
    """An empty channel allowlist selects nothing."""

    model_config = ConfigDict(extra="forbid", frozen=True)
    team_id: StrictStr = Field(pattern=r"^T[A-Z0-9]{2,}$")
    token_env: StrictStr = Field(
        default="SLACK_TOKEN", pattern=r"^[A-Za-z_][A-Za-z0-9_]*$"
    )
    state_dir: Path
    channels: tuple[ChannelId, ...]
    history_days: StrictInt | None = Field(default=None, gt=0, le=365000)
    thread_days: StrictInt = Field(default=7, gt=0, le=365000)
    poll_seconds: float = Field(default=300, gt=0, allow_inf_nan=False)
    destinations: tuple[DestinationConfig, ...]

    @property
    def source_id(self) -> str:
        return f"slack:{self.team_id}"

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
