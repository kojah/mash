"""Concrete delivery destinations; connector code does not run inside Mash."""

import hashlib
import math
import re
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Self

# The same rules Mash applies to `[shared]` in its TOML configuration, so a
# destination accepted here names a node the server can be configured with.
IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,254}")
TOKEN_ENV = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


@dataclass(frozen=True)
class Local:
    """Local silver written through the `mash` executable."""

    database: Path
    executable: str
    config: Path | None


@dataclass(frozen=True)
class Shared:
    """One node's shared silver behind the authenticated Mash HTTP API.

    The token is read from `token_env` at each delivery and is never stored.
    """

    api_url: str
    group_id: str
    node_id: str
    token_env: str


@dataclass(frozen=True)
class Destination:
    target: Local | Shared
    timeout: float

    @classmethod
    def local(
        cls,
        *,
        database: Path,
        executable: str = "mash",
        config: Path | None = None,
        timeout: float = 600,
    ) -> Self:
        """Select local silver; timeout is seconds, not an indexing deadline.

        A batch into a large local index can take tens of seconds; a timeout
        stops Mash mid-write and the batch is retried, so err long.
        """
        return cls(Local(database.resolve(), executable, config), checked(timeout))

    @classmethod
    def shared(
        cls,
        *,
        api_url: str,
        group_id: str,
        node_id: str,
        token_env: str,
        timeout: float = 30,
    ) -> Self:
        """Select a node's shared silver.

        `group_id` and `node_id` must match the server's configuration for the
        token, which the server enforces. Timeout is seconds per socket
        operation, so a slowly trickling response can take longer in total.
        """
        url = urllib.parse.urlsplit(api_url)
        if (
            url.scheme not in {"http", "https"}
            or not url.hostname
            or url.username is not None
            or url.password is not None
            or url.query
            or url.fragment
        ):
            raise ValueError(
                "shared api_url must be HTTP(S), without credentials, query, "
                "or fragment"
            )
        for value, label in ((group_id, "group ID"), (node_id, "node ID")):
            if not IDENTIFIER.fullmatch(value):
                raise ValueError(f"invalid {label}")
        if not TOKEN_ENV.fullmatch(token_env):
            raise ValueError("invalid token_env")
        target = Shared(api_url.rstrip("/"), group_id, node_id, token_env)
        return cls(target, checked(timeout))

    def _identity(self) -> str:
        # Ordering and checkpoints belong to the receiving silver database, so
        # binary, config, or token changes keep the identity while separate
        # databases or nodes must never share it.
        match self.target:
            case Local(database=database):
                digest = hashlib.sha256(str(database).encode()).hexdigest()
                return f"local:{digest}"
            case Shared(api_url=api_url, group_id=group_id, node_id=node_id):
                key = "\0".join((api_url, group_id, node_id))
                return f"shared:{hashlib.sha256(key.encode()).hexdigest()}"


def checked(timeout: float) -> float:
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("delivery timeout must be finite and positive")
    return timeout
