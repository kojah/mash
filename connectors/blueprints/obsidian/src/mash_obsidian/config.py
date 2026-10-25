"""Strict TOML configuration; credentials remain in destination environments."""

import tomllib
from pathlib import Path, PurePosixPath

from mash_sdk import Destination
from mash_sdk.config import DestinationConfig, Local, absolute
from pydantic import BaseModel, ConfigDict, Field, StrictBool, StrictInt, StrictStr


class Settings(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    vault: Path
    vault_id: StrictStr = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$")
    state_dir: Path
    folders: tuple[StrictStr, ...]
    recursive: StrictBool = True
    history_days: StrictInt | None = Field(default=None, gt=0, le=365000)
    poll_seconds: float = Field(default=5, gt=0, allow_inf_nan=False)
    deletion_grace_seconds: float = Field(default=2, ge=0, allow_inf_nan=False)
    max_note_bytes: StrictInt = Field(default=1_000_000, gt=0)
    destinations: tuple[DestinationConfig, ...]

    @property
    def source_id(self) -> str:
        return f"obsidian:{self.vault_id}"

    def selected(self, relative: str) -> bool:
        path = PurePosixPath(relative)
        if any(part in {".obsidian", ".trash", ".git"} for part in path.parts):
            return False
        for folder in self.folders:
            root = PurePosixPath(folder)
            if path.is_relative_to(root):
                tail = path.relative_to(root)
                if self.recursive or len(tail.parts) == 1:
                    return True
        return False

    def targets(self) -> tuple[Destination, ...]:
        return tuple(item.destination() for item in self.destinations)


def load(path: Path, *, validate_paths: bool = True) -> Settings:
    with path.open("rb") as stream:
        settings = Settings.model_validate(tomllib.load(stream))
    base = path.resolve().parent

    targets = tuple(item.relative_to(base) for item in settings.destinations)
    settings = settings.model_copy(
        update={
            "vault": absolute(settings.vault, base),
            "state_dir": absolute(settings.state_dir, base),
            "destinations": targets,
        }
    )
    if validate_paths and not settings.vault.is_dir():
        raise ValueError("vault must be an existing directory")
    if settings.state_dir.is_relative_to(settings.vault):
        raise ValueError("connector state must stay outside the vault")
    if any(
        isinstance(target, Local) and target.database.is_relative_to(settings.vault)
        for target in settings.destinations
    ):
        raise ValueError("Mash databases must stay outside the vault")
    if not settings.folders or not settings.destinations:
        raise ValueError("select at least one folder and destination")
    for folder in settings.folders:
        name = PurePosixPath(folder)
        if name.is_absolute() or ".." in name.parts or "\\" in folder:
            raise ValueError("folders must be vault-relative paths without traversal")
        if name.as_posix() != folder or not folder:
            raise ValueError("folders must use normalized paths; use . for vault root")
    settings.targets()  # Validate destination identifiers, URLs and token names now.
    return settings
