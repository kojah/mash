"""Read-only setup checks for runnable connectors; they never ingest, install or write.

A connector package ships a versioned `prerequisites.toml` declaring each
requirement's `id`, `when` (`always`, `local` or `shared`), description and
remedy. The connector checks its own source requirements; `check` adds Python
and the destination requirements `mash` and `database` (per local destination)
and `credentials` and `authority` (per shared destination), which the manifest
must declare with connector-specific wording.
"""

import os
import shutil
import sqlite3
import subprocess
import sys
import tomllib
from collections.abc import Mapping, Sequence
from contextlib import closing
from enum import StrEnum
from importlib.resources import files
from pathlib import Path
from typing import Literal, Self

import orjson
from pydantic import BaseModel, ConfigDict, model_validator

from .destination import Destination, Local


class Requirement(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    id: str
    when: Literal["always", "local", "shared"]
    description: str
    remedy: str


class Manifest(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    version: Literal[1]
    connector: str
    python: tuple[int, int]
    requirements: tuple[Requirement, ...]

    @model_validator(mode="after")
    def unique(self) -> Self:
        if len({item.id for item in self.requirements}) != len(self.requirements):
            raise ValueError("requirement IDs must be unique")
        return self

    @classmethod
    def load(cls, package: str) -> Self:
        """The `prerequisites.toml` shipped inside an installed package."""
        text = files(package).joinpath("prerequisites.toml").read_text()
        return cls.model_validate(tomllib.loads(text))


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


def passed(ok: bool) -> Status:
    return Status.passed if ok else Status.failed


def writable(path: Path) -> bool:
    """Whether this user could create or update `path`, without touching it."""
    if path.exists():
        access = os.R_OK | os.W_OK | (os.X_OK if path.is_dir() else 0)
        return os.access(path, access)
    parent = path.parent
    while not parent.exists() and parent != parent.parent:
        parent = parent.parent
    return parent.is_dir() and os.access(parent, os.W_OK | os.X_OK)


def writable_state(directory: Path, *names: str) -> bool:
    """A state directory (existing or creatable) whose named files are writable."""
    return (
        writable(directory)
        and (not directory.exists() or directory.is_dir())
        and all(writable(directory / name) for name in names)
    )


def silver_schema(executable: str) -> int | None:
    """The silver schema an installed Mash reports, or None if it cannot run."""
    found = shutil.which(executable)
    if not found:
        return None
    try:
        response = subprocess.run(
            [found, "--version"], capture_output=True, timeout=5, check=True
        )
        value = orjson.loads(response.stdout)
    except (OSError, subprocess.SubprocessError, orjson.JSONDecodeError):
        return None
    schema = value.get("silver_schema") if isinstance(value, dict) else None
    return schema if type(schema) is int else None


def compatible_database(database: Path, schema: int | None) -> bool:
    """A missing database is created on delivery; an existing one must match."""
    if not database.exists():
        return True
    if not database.is_file() or schema is None:
        return False
    try:
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as db:
            return db.execute("PRAGMA user_version").fetchone()[0] == schema
    except sqlite3.Error:
        return False


def check(
    manifest: Manifest,
    source: Mapping[str, Status],
    destinations: Sequence[Destination],
) -> Report:
    """Python, then the connector's `source` results in order, then destinations.

    Makes no network requests, so shared authority is always `unverified`: a
    token's presence does not prove the server accepts it for this node.
    """
    declared = {item.id: item for item in manifest.requirements}
    checks: list[Check] = []

    def add(key: str, status: Status, destination: int | None = None) -> None:
        requirement = declared[key]
        checks.append(
            Check(
                requirement=key,
                destination=destination,
                status=status,
                description=requirement.description,
                remedy=None if status == Status.passed else requirement.remedy,
            )
        )

    add("python", passed(sys.version_info[:2] >= manifest.python))
    for key, status in source.items():
        add(key, status)
    for index, destination in enumerate(destinations):
        target = destination.target
        if isinstance(target, Local):
            schema = silver_schema(target.executable)
            config_readable = target.config is None or (
                target.config.is_file() and os.access(target.config, os.R_OK)
            )
            add("mash", passed(schema is not None and config_readable), index)
            add(
                "database",
                passed(
                    writable(target.database)
                    and writable(target.database.parent)
                    and compatible_database(target.database, schema)
                ),
                index,
            )
        else:
            add("credentials", passed(bool(os.environ.get(target.token_env))), index)
            add("authority", Status.unverified, index)
    return Report(
        connector=manifest.connector,
        ready=all(item.status != Status.failed for item in checks),
        fully_verified=all(item.status == Status.passed for item in checks),
        checks=checks,
    )


def emit(report: Report) -> int:
    """Write the report as one JSON line; return the exit code (2 if not ready)."""
    sys.stdout.buffer.write(orjson.dumps(report.model_dump(mode="json")) + b"\n")
    sys.stdout.flush()
    return 0 if report.ready else 2
