"""Deterministic quotas, distinct matching and per-history graph isolation."""

import unittest

from link_review import Candidate, Match, Reason, select


def candidate(
    identity: str, predictions: tuple[bool, ...], distance: int = 2, index: int = 40
) -> Candidate:
    return Candidate(
        identity, index, "a", "b", distance, True, "codex", "xenia", predictions
    )


class SelectionContracts(unittest.TestCase):
    def test_changed_cases_have_distinct_exact_matched_controls(self) -> None:
        changed = (
            candidate("add", (False, False, True, True)),
            candidate("remove", (True, True, False, False)),
        )
        stable = (candidate("false", (False,) * 4), candidate("true", (True,) * 4))
        draws = select(changed, stable)
        self.assertEqual(len(draws), 4)
        controls = [d for d in draws if Reason.CONTROL in d.reasons]
        self.assertEqual({d.control_for for d in controls}, {"add", "remove"})
        self.assertTrue(all(d.match == Match.EXACT for d in controls))
        self.assertEqual(len({d.pair_id for d in draws}), len(draws))
        self.assertEqual(
            draws, select(tuple(reversed(changed)), tuple(reversed(stable)))
        )

    def test_unmatched_and_nearest_fallback_are_explicit(self) -> None:
        changed = (candidate("add", (False, False, True, True)),)
        unmatched = select(changed, ())
        self.assertEqual(unmatched[0].match, Match.UNMATCHED)
        draws = select(changed, (candidate("far", (False,) * 4, distance=8),))
        self.assertEqual(next(d for d in draws if d.control_for).match, Match.NEAREST)

    def test_broader_match_preserves_provider_node_length_and_class(self) -> None:
        changed = (candidate("add", (False, False, True, True)),)
        draws = select(changed, (candidate("other", (False,) * 4, index=41),))
        control = next(d for d in draws if d.control_for)
        self.assertEqual(control.match, Match.BROADER)
        self.assertEqual(control.pair_id, "other")


if __name__ == "__main__":
    unittest.main()
