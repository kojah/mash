"""One-shot ingestion or in-process polling with content-free JSON diagnostics."""

import asyncio
import sqlite3
import sys
from pathlib import Path

import click
import orjson
from mash_sdk import DeliveryError
from mash_sdk.prerequisites import emit

from .config import load
from .connector import Result, run
from .prerequisites import check


@click.command()
@click.option(
    "--config", "path", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--watch", is_flag=True, help="Poll within this process; installs no service."
)
@click.option(
    "--check",
    "preflight",
    is_flag=True,
    help="Report prerequisites without ingesting or writing state.",
)
def main(path: Path, watch: bool, preflight: bool) -> None:
    """Ingest a selected Obsidian vault through Mash's external connector SDK."""
    incomplete = False

    def report(result: Result) -> None:
        nonlocal incomplete
        incomplete = bool(result.issues)
        sys.stdout.buffer.write(orjson.dumps(result.json()) + b"\n")
        sys.stdout.flush()

    try:
        settings = load(path, validate_paths=not preflight)
        if preflight:
            if watch:
                raise ValueError("check and watch are mutually exclusive")
            if code := emit(check(settings)):
                raise click.exceptions.Exit(code)
            return
        asyncio.run(run(settings, watch=watch, report=report))
    except KeyboardInterrupt:
        return
    except DeliveryError as error:
        sys.stderr.buffer.write(
            orjson.dumps(
                {
                    "error": {
                        "kind": "delivery",
                        "retryable": error.retryable,
                        "message": str(error),
                    }
                }
            )
            + b"\n"
        )
        raise click.exceptions.Exit(1) from None
    except (OSError, ValueError, sqlite3.Error):
        sys.stderr.buffer.write(
            b'{"error":{"kind":"configuration-or-state","retryable":false}}\n'
        )
        raise click.exceptions.Exit(2) from None
    if incomplete:
        raise click.exceptions.Exit(9)
