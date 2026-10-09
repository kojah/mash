"""Private, bounded-memory CLI delivery with no canonical text in errors."""

import subprocess
import tempfile
from pathlib import Path

from ._receipts import MAX_RECEIPT_BYTES
from .destination import Destination


class DeliveryError(RuntimeError):
    """Delivery failed; the saved batch remains available for recovery."""

    def __init__(self, message: str, *, retryable: bool) -> None:
        super().__init__(message)
        self.retryable = retryable


def deliver(destination: Destination, payload: bytes, state_path: Path) -> bytes:
    # Private files avoid putting canonical data in argv or shell interpolation.
    # Keeping stdout on disk also bounds RAM if a faulty executable prints more
    # than the receipt limit; stderr is discarded rather than exposing history.
    with tempfile.TemporaryDirectory(
        prefix=".mash-delivery-", dir=state_path.parent
    ) as directory:
        root = Path(directory)
        batch = root / "batch.json"
        batch.write_bytes(payload)
        command = [destination.executable]
        if destination.config is not None:
            command.extend(["--config", str(destination.config)])
        command.extend(
            [
                "--db",
                str(destination.database),
                "index",
                "--connector-batch",
                str(batch),
            ]
        )
        with (root / "receipt.json").open("w+b") as output:
            try:
                result = subprocess.run(
                    command,
                    stdin=subprocess.DEVNULL,
                    stdout=output,
                    stderr=subprocess.DEVNULL,
                    timeout=destination.timeout,
                    check=False,
                )
            except subprocess.TimeoutExpired:
                raise DeliveryError(
                    "local Mash delivery timed out", retryable=True
                ) from None
            except OSError:
                raise DeliveryError(
                    "local Mash executable is unavailable", retryable=True
                ) from None
            if result.returncode != 0:
                # Mash's permanent input/schema/conflict failures need caller
                # intervention. None of these failures discard the saved batch.
                raise DeliveryError(
                    "local Mash rejected the delivery",
                    retryable=result.returncode not in {2, 7, 9, 12},
                )
            output.seek(0)
            return output.read(MAX_RECEIPT_BYTES + 1)
