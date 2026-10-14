"""The Slack Web API client and its token; the token is never logged or stored.

slack_sdk sends the token as a header (redacted from its debug logs), retries
connection errors once and HTTP 429 up to three times after `Retry-After`.
"""

import os

from slack_sdk.http_retry.builtin_async_handlers import (
    AsyncConnectionErrorRetryHandler,
    AsyncRateLimitErrorRetryHandler,
)
from slack_sdk.web.async_client import AsyncWebClient

API = "https://slack.com/api/"


class TokenMissing(Exception):
    """The configured environment variable holds no token."""


def token(env: str) -> str:
    value = os.environ.get(env, "").strip()
    if not value:
        raise TokenMissing(f"set {env} to a Slack user (xoxp-) or bot (xoxb-) token")
    return value


def connect(secret: str, base_url: str = API) -> AsyncWebClient:
    return AsyncWebClient(
        token=secret,
        base_url=base_url,
        timeout=60,
        user_agent_prefix="mash-slack-connector",
        retry_handlers=[
            AsyncConnectionErrorRetryHandler(),
            AsyncRateLimitErrorRetryHandler(max_retry_count=3),
        ],
    )
