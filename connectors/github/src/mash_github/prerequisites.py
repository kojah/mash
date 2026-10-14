"""GitHub's source checks; the SDK adds the Python and destination checks.

Offline: whether GitHub accepts the token for each repository stays
`unverified` until `sync` reads them.
"""

from mash_sdk import prerequisites
from mash_sdk.prerequisites import Manifest, Report, Status, passed, writable_state

from .client import TokenMissing, token
from .config import Settings


def has_token(env: str) -> bool:
    try:
        token(env)
    except TokenMissing:
        return False
    return True


def check(settings: Settings) -> Report:
    return prerequisites.check(
        Manifest.load("mash_github"),
        {
            "token": passed(has_token(settings.token_env)),
            "github_access": Status.unverified,
            "state": passed(writable_state(settings.state_dir, "delivery.sqlite3")),
        },
        settings.targets(),
    )
