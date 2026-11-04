"""Strict TOML configuration; the Slack token lives in a private file `login` writes."""

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
    # The Slack user the token acts as. Two credentials in one workspace are two
    # sources, not one: cursors, rate limits and visibility follow the principal.
    principal: StrictStr = Field(pattern=r"^[UW][A-Z0-9]{2,}$")
    # The user token `mash-slack login` saved, readable only by this user.
    token_file: Path
    state_dir: Path
    channels: tuple[ChannelId, ...]
    history_days: StrictInt | None = Field(default=None, gt=0, le=365000)
    thread_days: StrictInt = Field(default=7, gt=0, le=365000)
    poll_seconds: float = Field(default=300, gt=0, allow_inf_nan=False)
    destinations: tuple[DestinationConfig, ...]

    @property
    def source_id(self) -> str:
        return f"slack:{self.team_id}:{self.principal}"

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
            "token_file": absolute(settings.token_file, base),
            "destinations": tuple(
                item.relative_to(base) for item in settings.destinations
            ),
        }
    )
    if not settings.destinations:
        raise ValueError("configure at least one destination")
    settings.targets()  # Validate destination identifiers, URLs and token names now.
    return settings
