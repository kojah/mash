"""Capture a consistent research database and native files without production writes."""

import hashlib
import os
import shutil
import sqlite3
from pathlib import Path

import click
import orjson
from pydantic import BaseModel, ConfigDict


class Source(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    conversation_id: str
    original_path: Path
    captured_path: Path
    sha256: str


class Snapshot(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    node: str
    sources: tuple[Source, ...]
    missing_sources: int


def capture(database: Path, output: Path, node: str) -> Snapshot:
    """SQLite backup includes committed WAL data; source files are a later capture.

    Native files can change during copying, so later ancestry checks remain
    required. The original database and provider files are never modified.
    """
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    (output / "sources").mkdir(mode=0o700)
    with (
        sqlite3.connect(f"{database.resolve().as_uri()}?mode=ro", uri=True) as original,
        sqlite3.connect(output / "mash.sqlite3") as copy,
    ):
        original.backup(copy)
        copy.execute("PRAGMA journal_mode=DELETE")
        rows = copy.execute(
            "SELECT id,source_path FROM conversations "
            "WHERE provider IN ('claude-code','codex') ORDER BY id"
        ).fetchall()
    (output / "mash.sqlite3").chmod(0o600)
    sources = []
    missing = 0
    for index, (identity, raw_path) in enumerate(rows):
        original_path = Path(raw_path)
        if not original_path.is_file():
            missing += 1
            continue
        relative = Path("sources") / f"{index}.jsonl"
        destination = output / relative
        shutil.copyfile(original_path, destination)
        destination.chmod(0o600)
        with destination.open("rb") as file:
            sha256 = hashlib.file_digest(file, "sha256").hexdigest()
        sources.append(
            Source(
                conversation_id=identity,
                original_path=original_path,
                captured_path=relative,
                sha256=sha256,
            )
        )
    snapshot = Snapshot(node=node, sources=tuple(sources), missing_sources=missing)
    (output / "snapshot.json").write_bytes(
        orjson.dumps(snapshot.model_dump(mode="json"))
    )
    (output / "snapshot.json").chmod(0o600)
    return snapshot


@click.command()
@click.option("--database", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--node", required=True)
def main(database: Path, output: Path, node: str) -> None:
    os.umask(0o077)
    snapshot = capture(database, output, node)
    click.echo(
        f"Captured {len(snapshot.sources)} sources; missing {snapshot.missing_sources}"
    )


if __name__ == "__main__":
    main()
