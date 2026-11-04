"""`sync` once or `--watch`; diagnostics carry counts and repositories, never tokens."""

import asyncio
import sqlite3
import sys
from pathlib import Path

import click
import orjson
from mash_sdk import DeliveryError
from mash_sdk.config import problems
from mash_sdk.prerequisites import emit

from .client import TokenMissing, connect, token
from .config import Settings, load
from .connector import Result, run
from .prerequisites import check


def fail(kind: str, exit_code: int, **details: object) -> click.exceptions.Exit:
    sys.stderr.buffer.write(orjson.dumps({"error": {"kind": kind, **details}}) + b"\n")
    return click.exceptions.Exit(exit_code)


@click.group(invoke_without_command=True)
@click.option(
    "--config", "path", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--check",
    "preflight",
    is_flag=True,
    help="Report prerequisites offline without ingesting or writing state.",
)
@click.pass_context
def main(context: click.Context, path: Path, preflight: bool) -> None:
    """Ingest selected GitHub issues, pull requests and reviews into Mash, read-only."""
    try:
        context.obj = load(path)
    except (OSError, ValueError) as error:
        raise fail(
            "configuration", 2, retryable=False, problems=problems(error)
        ) from None
    if preflight:
        if context.invoked_subcommand is not None:
            raise click.UsageError("--check runs alone, without a command")
        raise click.exceptions.Exit(emit(check(context.obj)))
    if context.invoked_subcommand is None:
        raise click.UsageError("choose a command: sync")


@main.command()
@click.option(
    "--watch", is_flag=True, help="Poll within this process; installs no service."
)
@click.pass_obj
def sync(settings: Settings, watch: bool) -> None:
    """Deliver updated issues and PRs once, or keep polling with --watch."""
    incomplete = False

    def report(result: Result) -> None:
        nonlocal incomplete
        incomplete = bool(result.issues)
        sys.stdout.buffer.write(orjson.dumps(result.json()) + b"\n")
        sys.stdout.flush()

    try:
        github = connect(token(settings.token_env))
        asyncio.run(run(settings, github, watch=watch, report=report))
    except KeyboardInterrupt:
        return
    except TokenMissing as error:
        raise fail("token-missing", 3, message=str(error)) from None
    except DeliveryError as error:
        raise fail("delivery", 1, retryable=error.retryable) from None
    except (OSError, ValueError, sqlite3.Error):
        raise fail("configuration-or-state", 2, retryable=False) from None
    if incomplete:
        raise click.exceptions.Exit(9)
