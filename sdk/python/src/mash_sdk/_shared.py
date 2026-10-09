"""Private HTTP delivery to shared silver with no canonical text or token in errors."""

import http.client
import os
import urllib.error
import urllib.request

from ._delivery import DeliveryError
from ._receipts import MAX_RECEIPT_BYTES
from .destination import Shared

# Rate limiting, request timeouts and server-side failures may succeed later.
# Other statuses (input, authorization, conflicts, limits) need intervention.
RETRYABLE_STATUSES = frozenset({408, 429, 500, 502, 503, 504})


class NoRedirects(urllib.request.HTTPRedirectHandler):
    # Following a redirect would resend the bearer token and canonical payload
    # to a location the configuration never named; Mash's own client refuses.
    def redirect_request(self, *args: object, **kwargs: object) -> None:
        return None


OPENER = urllib.request.build_opener(NoRedirects)


def deliver_shared(shared: Shared, timeout: float, payload: bytes) -> bytes:
    token = os.environ.get(shared.token_env)
    if not token:
        raise DeliveryError(
            f"set {shared.token_env} to this node's API token", retryable=False
        )
    if not token.isascii() or not token.isprintable() or " " in token:
        raise DeliveryError("invalid API token", retryable=False)
    request = urllib.request.Request(
        f"{shared.api_url}/v1/connector-batches",
        data=payload,
        method="POST",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "Accept": "application/json",
            # The server rejects a token issued for another node instead of
            # committing under it, so ledgers for different nodes never mix.
            "x-mash-group": shared.group_id,
            "x-mash-node": shared.node_id,
        },
    )
    try:
        with OPENER.open(request, timeout=timeout) as response:
            return response.read(MAX_RECEIPT_BYTES + 1)
    except urllib.error.HTTPError as error:
        # The body may describe the request; never surface it.
        error.close()
        raise DeliveryError(
            f"shared Mash rejected the delivery (HTTP {error.code})",
            retryable=error.code in RETRYABLE_STATUSES,
        ) from None
    except (urllib.error.URLError, http.client.HTTPException, OSError):
        # Includes timeouts and dropped connections after the server may have
        # committed; retrying the saved batch replays the original receipt.
        raise DeliveryError("shared Mash is unreachable", retryable=True) from None
