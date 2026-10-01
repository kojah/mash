import unittest
from pathlib import Path
import tempfile

from review_packets import Candidate, CohortProvider, LengthStratum, exclusions, select, review_text


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

    def test_counts_and_seeds_are_bounded_and_strata_are_not_silently_dropped(self):
        for seed, count in (("", 1), ("seed", 0), ("seed", True), ("seed", 17), ("seed", 3)):
            with self.assertRaises(ValueError):
                select(self.candidates(), seed, count)
        self.assertEqual(len(select(self.candidates(), "new", 1)), 4)

    def test_exclusions_union_aliases_and_reject_generation_or_missing_group(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = tuple(Path(directory) / name for name in ("first.toml", "second.toml"))
            for path, identity in zip(paths, ("old", "alias")):
                path.write_text(f'schema_version = 1\ngeneration = "g"\n[[cases]]\nthread = "{identity}"\n')
            groups = {"old": [1] * 32, "alias": [1] * 32}
            excluded, manifest = exclusions(paths, groups, "g")
            self.assertEqual(excluded, {bytes([1]) * 32})
            self.assertEqual(len(manifest), 2)
            with self.assertRaises(ValueError):
                exclusions(paths, groups, "changed")
            with self.assertRaises(ValueError):
                exclusions(paths, {"old": [1] * 32}, "g")
