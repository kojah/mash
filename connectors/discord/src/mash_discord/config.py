"""Strict TOML configuration; Discord and Mash tokens stay in the environment."""

import tomllib
from datetime import datetime, timedelta
from pathlib import Path
from typing import Annotated

from mash_sdk import Destination
from mash_sdk.config import DestinationConfig, absolute
from pydantic import BaseModel, ConfigDict, Field, StrictInt, StrictStr

# A channel or thread snowflake; the pattern also keeps it inert in URLs.
ChannelId = Annotated[StrictStr, Field(pattern=r"^[0-9]{1,20}$")]


class Settings(BaseModel):
    """An empty allowlist selects nothing; a thread is selected by its own ID."""

    model_config = ConfigDict(extra="forbid", frozen=True)
    account_id: StrictStr = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$")
    token_env: StrictStr = Field(
        default="DISCORD_BOT_TOKEN", pattern=r"^[A-Za-z_][A-Za-z0-9_]*$"
    )
    state_dir: Path
    channels: tuple[ChannelId, ...]
    history_days: StrictInt | None = Field(default=None, gt=0, le=365000)
    poll_seconds: float = Field(default=60, gt=0, allow_inf_nan=False)
    destinations: tuple[DestinationConfig, ...]

    @property
    def source_id(self) -> str:
        return f"discord:{self.account_id}"

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
