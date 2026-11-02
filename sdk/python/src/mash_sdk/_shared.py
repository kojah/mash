"""Private socket delivery through the mash executable; no token in argv."""

import os
import subprocess
import tempfile
from pathlib import Path

from ._delivery import DeliveryError
from ._receipts import MAX_RECEIPT_BYTES
from .destination import Shared

# Permanent input/schema/conflict failures need caller intervention; anything
# else (capacity, network, timeout) may succeed on a later run.
PERMANENT_EXIT_CODES = frozenset({2, 7, 9, 12})


def executable() -> str:
    return os.environ.get("MASH_EXECUTABLE", "mash")


def config_document(shared: Shared) -> str:
    """A minimal node configuration naming only the shared destination."""
    return (
        "version = 1\n"
        "[providers]\n"
        "[shared]\n"
        f'group_id = "{shared.group_id}"\n'
        f'node_id = "{shared.node_id}"\n'
        f'api_url = "{shared.api_url}"\n'
        f'token_env = "{shared.token_env}"\n'
    )


def deliver_shared(shared: Shared, timeout: float, payload: bytes) -> bytes:
    if not os.environ.get(shared.token_env):
        raise DeliveryError(
            f"set {shared.token_env} to this node's API token", retryable=False
        )
    # Private files keep canonical data out of argv and bound memory if a faulty
    # executable prints more than a receipt. The token stays in the environment.
    with tempfile.TemporaryDirectory(prefix=".mash-delivery-") as directory:
        root = Path(directory)
        config = root / "mash.toml"
        config.write_text(config_document(shared))
        batch = root / "batch.json"
        batch.write_bytes(payload)
        command = [
            executable(),
            "--config",
            str(config),
            "index",
            "--connector-batch",
            str(batch),
            "--shared",
        ]
        with (root / "receipt.json").open("w+b") as output:
            try:
                result = subprocess.run(
                    command,
                    stdin=subprocess.DEVNULL,
                    stdout=output,
                    stderr=subprocess.DEVNULL,
                    timeout=timeout,
                    check=False,
                )
            except subprocess.TimeoutExpired:
                raise DeliveryError(
                    "shared Mash delivery timed out", retryable=True
                ) from None
            except OSError:
                raise DeliveryError(
                    "shared Mash executable is unavailable", retryable=True
                ) from None
            if result.returncode != 0:
                raise DeliveryError(
                    "shared Mash rejected the delivery",
                    retryable=result.returncode not in PERMANENT_EXIT_CODES,
                )
            output.seek(0)
            return output.read(MAX_RECEIPT_BYTES + 1)
