"""Durable identities, not note contents or a second delivery journal."""

import fcntl
import os
import sqlite3
from dataclasses import dataclass
from pathlib import Path
from types import TracebackType
from typing import Self

VERSION = 1
APPLICATION = 0x4D4F4253
SCHEMA = """
CREATE TABLE vault (id TEXT NOT NULL);
CREATE TABLE notes (
 id TEXT PRIMARY KEY, path TEXT NOT NULL, device INTEGER NOT NULL,
 inode INTEGER NOT NULL, digest TEXT NOT NULL, created_at INTEGER,
 missing_since REAL, active INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX active_paths ON notes(path) WHERE active=1;
"""


@dataclass(frozen=True)
class Identity:
    id: str
    path: str
    device: int
    inode: int
    digest: str
    created_at: int | None
    missing_since: float | None


class Registry:
    def __init__(self, directory: Path, vault_id: str) -> None:
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.lease = (directory / "connector.lock").open("a+b")
        try:
            fcntl.flock(self.lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            self.lease.close()
            raise ValueError("another Obsidian connector is active") from None
        database = directory / "identities.sqlite3"
        existed = database.exists()
        try:
            self.connection = sqlite3.connect(database, check_same_thread=False)
            if not existed:
                os.chmod(database, 0o600)
            version = self.connection.execute("PRAGMA user_version").fetchone()[0]
            app = self.connection.execute("PRAGMA application_id").fetchone()[0]
            if (
                version == 0
                and not self.connection.execute(
                    "SELECT 1 FROM sqlite_master WHERE type='table'"
                ).fetchone()
            ):
                self.connection.executescript(SCHEMA)
                self.connection.execute("INSERT INTO vault VALUES(?)", [vault_id])
                self.connection.execute(f"PRAGMA application_id={APPLICATION}")
                self.connection.execute(f"PRAGMA user_version={VERSION}")
                self.connection.commit()
            elif version != VERSION or app != APPLICATION:
                raise ValueError("unsupported Obsidian identity registry")
            if self.connection.execute("SELECT id FROM vault").fetchone() != (
                vault_id,
            ):
                raise ValueError("identity registry belongs to another vault")
        except BaseException:
            if hasattr(self, "connection"):
                self.connection.close()
            self.lease.close()
            raise

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.connection.close()
        self.lease.close()

    def active(self) -> list[Identity]:
        return [
            Identity(*row)
            for row in self.connection.execute(
                "SELECT id,path,device,inode,digest,created_at,missing_since "
                "FROM notes WHERE active=1 ORDER BY id"
            )
        ]

    def observe(
        self, notes: list[Identity], missing: list[Identity], now: float
    ) -> None:
        with self.connection:
            self.connection.executemany(
                "INSERT INTO notes(id,path,device,inode,digest,created_at) "
                "VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET "
                "path=excluded.path,device=excluded.device,inode=excluded.inode,"
                "digest=excluded.digest,created_at=excluded.created_at,"
                "missing_since=NULL,active=1",
                [
                    (n.id, n.path, n.device, n.inode, n.digest, n.created_at)
                    for n in notes
                ],
            )
            self.connection.executemany(
                "UPDATE notes SET missing_since=COALESCE(missing_since,?) WHERE id=?",
                [(now, n.id) for n in missing],
            )

    def deleted(self, ids: list[str]) -> None:
        # Only after every configured destination acknowledges the SDK delivery.
        with self.connection:
            self.connection.executemany(
                "UPDATE notes SET active=0 WHERE id=?", [(id,) for id in ids]
            )
