"""Obsidian's source checks; the SDK adds the Python and destination checks."""

import os
from pathlib import Path

from mash_sdk import prerequisites
from mash_sdk.prerequisites import Manifest, Report, passed, writable_state

from .config import Settings


def manifest() -> Manifest:
    return Manifest.load("mash_obsidian")


def readable(path: Path) -> bool:
    return path.is_dir() and os.access(path, os.R_OK | os.X_OK)


def folder_readable(vault: Path, folder: str) -> bool:
    path = vault / folder
    # The vault itself may sit behind a symlink; nothing inside it may.
    inside = (part for part in (path, *path.parents) if part.is_relative_to(vault))
    return readable(path) and not any(part.is_symlink() for part in inside)


def check(settings: Settings) -> Report:
    vault = readable(settings.vault) and all(
        folder_readable(settings.vault, folder) for folder in settings.folders
    )
    state = writable_state(
        settings.state_dir, "identities.sqlite3", "delivery.sqlite3", "connector.lock"
    )
    return prerequisites.check(
        manifest(),
        {"vault": passed(vault), "state": passed(state)},
        settings.targets(),
    )
