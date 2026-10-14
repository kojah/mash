"""Google Docs' source checks; the SDK adds the Python and destination checks.

Offline: no token refresh is attempted, so Google's acceptance of the stored
authorization stays `unverified` until `sync` refreshes it.
"""

from pathlib import Path

from mash_sdk import prerequisites
from mash_sdk.prerequisites import Manifest, Report, Status, passed, writable_state
from pydantic import ValidationError

from .auth import SCOPE, AuthorizationRequired, load_client, load_token
from .config import Settings


def private(path: Path) -> bool:
    return path.is_file() and not path.stat().st_mode & 0o077


def client_ready(path: Path) -> bool:
    try:
        load_client(path)
    except (OSError, ValueError):
        return False
    return private(path)


def token_ready(path: Path) -> bool:
    try:
        # Rejects missing and group- or world-readable files itself.
        return load_token(path).scope == SCOPE
    except (OSError, AuthorizationRequired, ValidationError):
        return False


def check(settings: Settings) -> Report:
    return prerequisites.check(
        Manifest.load("mash_google_docs"),
        {
            "client_secret": passed(client_ready(settings.client_secret)),
            "token": passed(token_ready(settings.token)),
            "authorization": Status.unverified,
            "state": passed(writable_state(settings.state_dir, "delivery.sqlite3")),
        },
        settings.targets(),
    )
