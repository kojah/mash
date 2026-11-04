"""`sync` once or `--watch`; diagnostics carry counts and channel IDs, never tokens."""

import asyncio
import sqlite3
import sys
import webbrowser
from pathlib import Path

import click
import orjson
from mash_sdk import DeliveryError
from mash_sdk.config import problems
from mash_sdk.prerequisites import emit

from . import login
from .client import TokenMissing, connect, token
from .config import Settings, load
from .connector import Result, WrongWorkspace, run
from .prerequisites import check


def fail(kind: str, exit_code: int, **details: object) -> click.exceptions.Exit:
    sys.stderr.buffer.write(orjson.dumps({"error": {"kind": kind, **details}}) + b"\n")
    return click.exceptions.Exit(exit_code)


@click.group(invoke_without_command=True)
@click.option("--config", "path", type=click.Path(path_type=Path), required=True)
@click.option(
    "--check",
    "preflight",
    is_flag=True,
    help="Report prerequisites offline without ingesting or writing state.",
)
@click.pass_context
def main(context: click.Context, path: Path, preflight: bool) -> None:
    """Ingest selected Slack channels, threads included, into Mash, read-only."""
    if context.invoked_subcommand == "login" and not preflight:
        # Login creates the configuration, so it may not exist yet.
        context.obj = path
        return
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
        raise click.UsageError("choose a command: login or sync")


@main.command()
@click.option(
    "--watch", is_flag=True, help="Poll within this process; installs no service."
)
@click.pass_obj
def sync(settings: Settings, watch: bool) -> None:
    """Deliver new and changed messages once, or keep polling with --watch."""
    incomplete = False

    def report(result: Result) -> None:
        nonlocal incomplete
        incomplete = bool(result.issues)
        sys.stdout.buffer.write(orjson.dumps(result.json()) + b"\n")
        sys.stdout.flush()

    try:
        slack = connect(token(settings.token_file))
        asyncio.run(run(settings, slack, watch=watch, report=report))
    except KeyboardInterrupt:
        return
    except TokenMissing as error:
        raise fail("token-missing", 3, message=str(error)) from None
    except WrongWorkspace as error:
        raise fail("wrong-workspace", 2, message=str(error)) from None
    except DeliveryError as error:
        raise fail(
            "delivery", 1, retryable=error.retryable, message=str(error)
        ) from None
    except (OSError, ValueError, sqlite3.Error):
        raise fail("configuration-or-state", 2, retryable=False) from None
    if incomplete:
        raise click.exceptions.Exit(9)


@main.command("login")
@click.option(
    "--client-id",
    help="Client ID of the Mash app in your workspace (Basic Information).",
)
@click.option("--port", default=login.PORT, show_default=True, type=int)
@click.option("--no-browser", is_flag=True, help="Print links instead of opening.")
@click.pass_obj
def login_command(
    path: Path, client_id: str | None, port: int, no_browser: bool
) -> None:
    """Sign in to Slack in the browser and save a token; creates --config if new."""

    def show(url: str) -> None:
        click.echo(url, err=True)
        if not no_browser:
            webbrowser.open(url)

    if client_id is None:
        click.echo(
            "Create Mash's Slack app in your workspace (once): pick the "
            "workspace, review the read-only permissions and click Create:",
            err=True,
        )
        show(login.create_app_url(port))
        client_id = click.prompt(
            "Then paste its Client ID from Basic Information", err=True
        ).strip()
    click.echo("Approve Mash in the browser:", err=True)
    try:
        outcome = login.run(path, client_id, port, show)
    except login.LoginFailed as error:
        raise fail("login", 2, message=str(error)) from None
    except (OSError, ValueError) as error:
        raise fail("login", 2, problems=problems(error)) from None
    next_steps = ["Restart any running mash-slack sync to use the new token."]
    if outcome.created_config:
        next_steps = [
            f"Add the channel IDs to index to channels in {path} "
            "(View channel details shows each ID), then run sync."
        ]
    report = {
        "team_id": outcome.login.team_id,
        "principal": outcome.login.principal,
        "config": str(path),
        "created_config": outcome.created_config,
        "token_file": str(outcome.token_file),
        "next": next_steps,
    }
    sys.stdout.buffer.write(orjson.dumps(report) + b"\n")
