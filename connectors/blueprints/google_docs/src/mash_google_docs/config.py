"""Strict TOML configuration; Google and Mash credentials stay outside it."""

import tomllib
from pathlib import Path
from typing import Annotated

from mash_sdk import Destination
from mash_sdk.config import DestinationConfig, absolute
from pydantic import BaseModel, ConfigDict, Field, StrictInt, StrictStr

# Drive IDs are URL-safe base64; the pattern also keeps them inert in queries.
DriveId = Annotated[StrictStr, Field(pattern=r"^[A-Za-z0-9_-]{1,200}$")]


class Settings(BaseModel):
    """Allowlists select Docs; each empty allowlist selects nothing."""

    model_config = ConfigDict(extra="forbid", frozen=True)
    account_id: StrictStr = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$")
    client_secret: Path
    token: Path = Path("~/.config/mash/google-docs-token.json")
    state_dir: Path
    folders: tuple[DriveId, ...] = ()
    shared_drives: tuple[DriveId, ...] = ()
    documents: tuple[DriveId, ...] = ()
    history_days: StrictInt | None = Field(default=None, gt=0, le=365000)
    poll_seconds: float = Field(default=300, gt=0, allow_inf_nan=False)
    destinations: tuple[DestinationConfig, ...]

    @property
    def source_id(self) -> str:
        return f"google-docs:{self.account_id}"

    def targets(self) -> tuple[Destination, ...]:
        return tuple(item.destination() for item in self.destinations)


def load(path: Path) -> Settings:
    with path.open("rb") as stream:
        settings = Settings.model_validate(tomllib.load(stream))
    base = path.resolve().parent

    settings = settings.model_copy(
        update={
            "client_secret": absolute(settings.client_secret, base),
            "token": absolute(settings.token, base),
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
