import unittest

from review_packets import Candidate, CohortProvider, LengthStratum, select, review_text


class SamplingTests(unittest.TestCase):
    def candidates(self):
        return [Candidate(f"{provider}-{stratum}-{i}", provider,
                          bytes([i * 4 + offset + 1]) * 32, stratum)
                for offset, (provider, stratum) in enumerate([
                    (CohortProvider.CLAUDE, LengthStratum.MEDIUM), (CohortProvider.CLAUDE, LengthStratum.SHORT),
                    (CohortProvider.CODEX, LengthStratum.MEDIUM), (CohortProvider.CODEX, LengthStratum.SHORT)])
                for i in range(3) if (i * 4 + offset + 1) % 5 != 0]

    def test_selection_preserves_strata_and_is_order_independent(self):
        candidates = self.candidates()
        result = select(candidates, "frozen")
        self.assertEqual(result, select(list(reversed(candidates)), "frozen"))
        self.assertEqual(len(result), 8)
        self.assertEqual(len({c.group for c in result}), 8)

    def test_reserved_and_duplicate_source_groups_rejected(self):
        candidates = self.candidates()
        for extra in (Candidate("reserved", CohortProvider.CODEX, bytes(32), LengthStratum.SHORT), candidates[0]):
            with self.assertRaises(ValueError):
                select(candidates + [extra], "frozen")

    def test_review_display_redacts_standalone_values_without_rewriting_sources(self):
        sensitive = "aB_9" * 20 + "#" + "cD_8" * 12
        display, redacted = review_text(sensitive)
        self.assertTrue(redacted)
        self.assertNotIn(sensitive, display)
        text = "Investigate the channel ownership deadlock."
        self.assertEqual(review_text(text), (text, False))
