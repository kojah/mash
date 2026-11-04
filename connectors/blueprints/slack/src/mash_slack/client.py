"""The Slack Web API client and its token; the token is never logged or copied.

slack_sdk sends the token as a header (redacted from its debug logs), retries
connection errors once and HTTP 429 up to three times after `Retry-After`.
"""

from pathlib import Path

from slack_sdk.http_retry.builtin_async_handlers import (
    AsyncConnectionErrorRetryHandler,
    AsyncRateLimitErrorRetryHandler,
)
from slack_sdk.web.async_client import AsyncWebClient

API = "https://slack.com/api/"


class TokenMissing(Exception):
    """No token has been saved in the configured token file."""


def token(path: Path) -> str:
    try:
        value = path.read_text().strip()
    except FileNotFoundError:
        value = ""
    if not value:
        raise TokenMissing(f"no Slack token in {path}; run mash-slack login")
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
