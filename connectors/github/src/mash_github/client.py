"""The GitHub REST client and its token; the token is never logged or stored.

githubkit retries rate-limited requests (primary and secondary limits, honouring
Retry-After) once and 5xx responses three times, and serves unchanged pages
from its in-memory HTTP cache through conditional requests.
"""

import os
import subprocess

import httpx
from githubkit import GitHub, TokenAuthStrategy


class TokenMissing(Exception):
    """Neither the configured environment variable nor `gh auth token` has a token."""


def token(env: str) -> str:
    """The token in `env`, else the GitHub CLI's; no network request is made."""
    value = os.environ.get(env)
    if value:
        return value
    try:
        result = subprocess.run(
            ["gh", "auth", "token"],
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        )
    except (OSError, subprocess.SubprocessError):
        raise TokenMissing(f"set {env} or log in with `gh auth login`") from None
    value = result.stdout.strip()
    if not value:
        raise TokenMissing(f"set {env} or log in with `gh auth login`")
    return value


def connect(
    secret: str, transport: httpx.AsyncBaseTransport | None = None
) -> GitHub[TokenAuthStrategy]:
    return GitHub(
        TokenAuthStrategy(secret),
        user_agent="mash-github-connector",
        timeout=60,
        async_transport=transport,
    )
