"""`auth` once, then `sync`; diagnostics carry counts and IDs, never text or tokens."""

import asyncio
import sqlite3
import sys
import urllib.request
from pathlib import Path

import click
import orjson
from mash_sdk import DeliveryError
from mash_sdk.config import problems
from mash_sdk.prerequisites import emit
from pydantic import ValidationError

from .api import HttpError, fetch, urlopen
from .auth import AuthorizationRequired, Credentials, authorize, load_client, load_token
from .config import Settings, load
from .connector import Result, run
from .drive import Drive
from .prerequisites import check


class WrongPrincipal(Exception):
    """The token belongs to a different Google account than `account_id`."""


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
    """Ingest selected Google Docs and their comments into Mash, read-only."""
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
        raise click.UsageError("choose a command: auth or sync")


@main.command()
@click.pass_obj
def auth(settings: Settings) -> None:
    """Grant read-only Drive access once; works over SSH by pasting the redirect."""

    def ask(url: str) -> str:
        click.echo(
            "Open this URL in any browser and approve read-only access:\n", err=True
        )
        click.echo(url + "\n", err=True)
        click.echo(
            "The browser then fails to load a 127.0.0.1 page; that is expected.",
            err=True,
        )
        return click.prompt("Paste that page's full address", err=True)

    try:
        authorize(
            load_client(settings.client_secret), settings.token, ask, send=urlopen
        )
    except ValidationError:
        # Pydantic messages echo their input, which here may hold a token.
        raise fail("authorization", 1, message="unexpected token response") from None
    except (ValueError, HttpError) as error:
        raise fail("authorization", 1, message=str(error)) from None
    click.echo(f"Stored the authorization in {settings.token} (mode 0600).", err=True)


@main.command()
@click.option(
    "--watch", is_flag=True, help="Poll within this process; installs no service."
)
@click.pass_obj
def sync(settings: Settings, watch: bool) -> None:
    """Deliver selected Docs once, or keep polling with --watch."""
    incomplete = False

    def report(result: Result) -> None:
        nonlocal incomplete
        incomplete = bool(result.issues)
        sys.stdout.buffer.write(orjson.dumps(result.json()) + b"\n")
        sys.stdout.flush()

    try:
        credentials = Credentials(
            load_client(settings.client_secret),
            load_token(settings.token),
            send=urlopen,
        )
        # Bind the credential to the source key before reading anything: the
        # key is google-docs:<sub>, so a token for another account must not
        # write here.
        request = urllib.request.Request("https://oauth2.googleapis.com/tokeninfo")
        request.add_header("Authorization", credentials.header())
        info = orjson.loads(fetch(urlopen, __import__("time").sleep, request))
        if info.get("sub") != settings.account_id:
            raise WrongPrincipal(
                f"the token belongs to {info.get('sub')}; "
                f"account_id is {settings.account_id}"
            )
        asyncio.run(
            run(settings, Drive(credentials, send=urlopen), watch=watch, report=report)
        )
    except KeyboardInterrupt:
        return
    except AuthorizationRequired as error:
        raise fail(
            "authorization-required",
            3,
            message=f"{error}; run `mash-google-docs --config ... auth` again",
        ) from None
    except DeliveryError as error:
        raise fail("delivery", 1, retryable=error.retryable) from None
    except HttpError as error:
        raise fail("google", 1, status=error.status) from None
    except (OSError, ValueError, sqlite3.Error):
        raise fail("configuration-or-state", 2, retryable=False) from None
    if incomplete:
        raise click.exceptions.Exit(9)
