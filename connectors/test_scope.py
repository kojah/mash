"""Selection tests run without the proposed SDK or live platform credentials."""

import unittest
from datetime import datetime, timedelta, timezone

from .scope import Scope


class ScopeTests(unittest.TestCase):
    def test_allowlist_applies_even_with_unbounded_history(self) -> None:
        scope = Scope(ids=("chosen",), history_days=None)
        self.assertTrue(scope.includes("chosen", None))
        self.assertFalse(scope.includes("other", None))
        self.assertFalse(Scope(ids=(), history_days=None).includes("chosen", None))

    def test_history_uses_creation_time_with_an_inclusive_boundary(self) -> None:
        now = datetime(2026, 10, 9, tzinfo=timezone.utc)
        scope = Scope(ids=("chosen",), history_days=90)
        cutoff = now - timedelta(days=90)
        self.assertEqual(scope.cutoff(now=now), cutoff)
        self.assertTrue(scope.includes("chosen", cutoff, now=now))
        self.assertFalse(
            scope.includes("chosen", cutoff - timedelta(milliseconds=1), now=now)
        )
        self.assertFalse(scope.includes("chosen", None, now=now))

    def test_invalid_windows_debounce_and_naive_timestamps_fail(self) -> None:
        for days in (0, -1, 365_001):
            with self.assertRaises(ValueError):
                Scope(ids=(), history_days=days)
        for quiet, maximum in (
            (-1, 10),
            (11, 10),
            (float("nan"), 10),
            (2, float("inf")),
        ):
            with self.assertRaises(ValueError):
                Scope(ids=(), quiet_seconds=quiet, max_wait_seconds=maximum)
        with self.assertRaises(ValueError):
            Scope(ids=("chosen",)).includes("chosen", datetime(2026, 10, 9))  # noqa: DTZ001 -- invalid input fixture
