"""Concrete delivery destinations; connector code does not run inside Mash."""

import hashlib
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Self


@dataclass(frozen=True)
class Destination:
    """A local Mash database. Shared HTTP delivery is not implemented yet."""

    database: Path
    executable: str
    config: Path | None
    timeout: float

    @classmethod
    def local(
        cls,
        *,
        database: Path,
        executable: str = "mash",
        config: Path | None = None,
        timeout: float = 30,
    ) -> Self:
        """Select local silver; timeout is seconds, not an indexing deadline."""
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("delivery timeout must be finite and positive")
        return cls(database.resolve(), executable, config, timeout)

    def _identity(self) -> str:
        # Binary replacements and config edits do not change the destination.
        # Separate local databases must never share ordering/checkpoints.
        digest = hashlib.sha256(str(self.database).encode()).hexdigest()
        return f"local:{digest}"
