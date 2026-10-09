"""Source selection shared by backfill and live updates."""

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from math import isfinite


@dataclass(frozen=True)
class Scope:
    """Allowlist, rolling history window, and bounded debounce.

    Empty IDs select nothing. Editing old messages does not widen the history.
    Changing scope never implicitly removes previously imported data.
    """

    ids: tuple[str, ...]
    history_days: int | None = 90
    quiet_seconds: float = 2
    max_wait_seconds: float = 10

    def __post_init__(self) -> None:
        if self.history_days is not None and not 0 < self.history_days <= 365_000:
            raise ValueError("history_days must be between 1 and 365000, or None")
        if (
            not isfinite(self.quiet_seconds)
            or not isfinite(self.max_wait_seconds)
            or self.quiet_seconds < 0
            or self.max_wait_seconds < self.quiet_seconds
        ):
            raise ValueError(
                "debounce requires finite 0 <= quiet_seconds <= max_wait_seconds"
            )

    def cutoff(self, *, now: datetime | None = None) -> datetime | None:
        if self.history_days is None:
            return None
        current = now if now is not None else datetime.now(timezone.utc)
        if current.tzinfo is None or current.utcoffset() is None:
            raise ValueError("source timestamps must include a timezone")
        return current - timedelta(days=self.history_days)

    def includes(
        self, id: str, created_at: datetime | None, *, now: datetime | None = None
    ) -> bool:
        if id not in self.ids:
            return False
        cutoff = self.cutoff(now=now)
        if cutoff is None:
            return True
        if created_at is None:
            return False
        if created_at.tzinfo is None or created_at.utcoffset() is None:
            raise ValueError("source timestamps must include a timezone")
        return created_at >= cutoff
