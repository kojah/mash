"""Google HTTP requests with bounded retries; bodies and tokens never enter errors."""

import re
import urllib.error
import urllib.request
from collections.abc import Callable
from email.message import Message

import orjson

# Five attempts wait at most 1 + 2 + 4 + 8 seconds, or the server's Retry-After
# capped at a minute, before the poll gives up and reports the failure.
ATTEMPTS = 5
MAX_WAIT_SECONDS = 60.0
OAUTH_CODE = re.compile(r"[a-z_]{1,64}")

# Returns status, lower-cased headers and body; HTTP errors are not raised.
Send = Callable[[urllib.request.Request], tuple[int, dict[str, str], bytes]]
Sleep = Callable[[float], None]


class HttpError(Exception):
    """A non-success Google response; the body is dropped except an OAuth error code."""

    def __init__(self, status: int, reason: str | None = None) -> None:
        super().__init__(f"Google API returned HTTP {status}")
        self.status = status
        self.reason = reason


def oauth_reason(body: bytes) -> str | None:
    """The RFC 6749 `error` code, such as invalid_grant; never free text."""
    try:
        value = orjson.loads(body)
    except orjson.JSONDecodeError:
        return None
    error = value.get("error") if isinstance(value, dict) else None
    return error if isinstance(error, str) and OAUTH_CODE.fullmatch(error) else None


def urlopen(request: urllib.request.Request) -> tuple[int, dict[str, str], bytes]:
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, lowered(response.headers), response.read()
    except urllib.error.HTTPError as error:
        return error.code, lowered(error.headers), error.read()


def lowered(headers: Message) -> dict[str, str]:
    return {name.lower(): value for name, value in headers.items()}


def retryable(status: int, body: bytes) -> bool:
    # Drive reports per-user quota exhaustion as 403 rate-limit reasons, not 429.
    return (
        status == 429
        or status >= 500
        or (status == 403 and b"ateLimitExceeded" in body)
    )


def fetch(send: Send, sleep: Sleep, request: urllib.request.Request) -> bytes:
    for attempt in range(ATTEMPTS):
        last = attempt == ATTEMPTS - 1
        try:
            status, headers, body = send(request)
        except OSError:  # Unreachable host, reset connection or socket timeout.
            if last:
                raise
            sleep(2.0**attempt)
            continue
        if 200 <= status < 300:
            return body
        if last or not retryable(status, body):
            raise HttpError(status, oauth_reason(body))
        sleep(wait(headers.get("retry-after"), attempt))
    raise AssertionError("unreachable")


def wait(retry_after: str | None, attempt: int) -> float:
    if retry_after is not None and retry_after.isdigit():
        return min(float(retry_after), MAX_WAIT_SECONDS)
    return 2.0**attempt
