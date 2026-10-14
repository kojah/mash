"""The Discord REST client and its bot token; the token is never logged or stored.

hikari's REST-only `RESTApp` never opens the gateway. It tracks Discord's
per-route rate-limit buckets and the global limit, waits out a 429's
`retry_after` (up to `MAX_RATE_LIMIT`; longer waits fail the channel), and
retries 5xx responses and connection errors three times with backoff.
"""

import os
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

import hikari
from hikari.api import RESTClient

MAX_RATE_LIMIT = 60.0


class TokenMissing(Exception):
    """The configured environment variable holds no bot token."""


def token(env: str) -> str:
    value = os.environ.get(env, "").strip()
    if not value:
        raise TokenMissing(f"set {env} to a Discord bot token")
    return value


@asynccontextmanager
async def connect(secret: str, url: str | None = None) -> AsyncIterator[RESTClient]:
    """`url` replaces Discord's API base, for tests."""
    app = hikari.RESTApp(max_rate_limit=MAX_RATE_LIMIT, url=url)
    await app.start()
    try:
        async with app.acquire(secret, hikari.TokenType.BOT) as rest:
            yield rest
    finally:
        await app.close()
