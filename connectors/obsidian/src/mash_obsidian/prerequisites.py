"""Declarative setup requirements and read-only checks; never ingest or install."""

import os
import shutil
import sqlite3
import subprocess
import sys
import tomllib
from contextlib import closing
from enum import StrEnum
from importlib.resources import files
from pathlib import Path
from typing import Literal

import orjson
from pydantic import BaseModel, ConfigDict

from .config import Local, Settings


class Requirement(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    id: str
    when: Literal["always", "local", "shared"]
    description: str
    remedy: str


class Manifest(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    version: Literal[1]
    connector: Literal["obsidian"]
    python: tuple[int, int]
    requirements: tuple[Requirement, ...]


class Status(StrEnum):
    passed = "passed"
    failed = "failed"
    unverified = "unverified"


class Check(BaseModel):
    requirement: str
    destination: int | None = None
    status: Status
    description: str
    remedy: str | None = None


class Report(BaseModel):
    connector: str
    ready: bool
    fully_verified: bool
    checks: list[Check]


def manifest() -> Manifest:
    return Manifest.model_validate(
        tomllib.loads(files("mash_obsidian").joinpath("prerequisites.toml").read_text())
    )


def writable(path: Path) -> bool:
    if path.exists():
        access = os.R_OK | os.W_OK | (os.X_OK if path.is_dir() else 0)
        return os.access(path, access)
    parent = path.parent
    while not parent.exists() and parent != parent.parent:
        parent = parent.parent
    return parent.is_dir() and os.access(parent, os.W_OK | os.X_OK)


def readable(path: Path) -> bool:
    return path.is_dir() and os.access(path, os.R_OK | os.X_OK)


def check(settings: Settings) -> Report:
    declared = manifest()
    checks: list[Check] = []

    def add(key: str, status: Status, destination: int | None = None) -> None:
        requirement = next(item for item in declared.requirements if item.id == key)
        checks.append(
            Check(
                requirement=key,
                destination=destination,
                status=status,
                description=requirement.description,
                remedy=None if status == Status.passed else requirement.remedy,
            )
        )

    def result(ok: bool) -> Status:
        return Status.passed if ok else Status.failed

    add("python", result(sys.version_info[:2] >= declared.python))
    add(
        "vault",
        result(
            readable(settings.vault)
            and all(
                readable(settings.vault / folder)
                and not any(
                    part.is_symlink()
                    for part in (
                        settings.vault / folder,
                        *(settings.vault / folder).parents,
                    )
                    if part.is_relative_to(settings.vault)
                )
                for folder in settings.folders
            )
        ),
    )
    add(
        "state",
        result(
            writable(settings.state_dir)
            and (not settings.state_dir.exists() or settings.state_dir.is_dir())
            and all(
                writable(settings.state_dir / name)
                for name in ("identities.sqlite3", "delivery.sqlite3", "connector.lock")
            )
        ),
    )
    for index, target in enumerate(settings.destinations):
        if isinstance(target, Local):
            executable = shutil.which(target.executable)
            schema: int | None = None
            if executable:
                try:
                    response = subprocess.run(
                        [executable, "--version"],
                        capture_output=True,
                        timeout=5,
                        check=True,
                    )
                    value = orjson.loads(response.stdout)
                    if (
                        isinstance(value, dict)
                        and type(value.get("silver_schema")) is int
                    ):
                        schema = value["silver_schema"]
                except (OSError, subprocess.SubprocessError, orjson.JSONDecodeError):
                    pass
            config_readable = target.config is None or (
                target.config.is_file() and os.access(target.config, os.R_OK)
            )
            add("mash", result(schema is not None and config_readable), index)
            compatible = not target.database.exists()
            if target.database.is_file() and schema is not None:
                try:
                    with closing(
                        sqlite3.connect(target.database.as_uri() + "?mode=ro", uri=True)
                    ) as db:
                        compatible = (
                            db.execute("PRAGMA user_version").fetchone()[0] == schema
                        )
                except sqlite3.Error:
                    compatible = False
            add(
                "database",
                result(
                    writable(target.database)
                    and writable(target.database.parent)
                    and compatible
                ),
                index,
            )
        else:
            add("credentials", result(bool(os.environ.get(target.token_env))), index)
            # No network requests: token presence does not prove remote authority.
            add("authority", Status.unverified, index)
    return Report(
        connector=declared.connector,
        ready=all(item.status != Status.failed for item in checks),
        fully_verified=all(item.status == Status.passed for item in checks),
        checks=checks,
    )
