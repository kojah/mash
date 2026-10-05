"""Ensure the sole enrichment batch preserves history and never resamples pairs."""

import hashlib
import os
import tempfile
import unittest
from pathlib import Path

from blind_turn_pairs import Packet
from blind_turn_pairs import freeze as initial_freeze
from corpus import Role
from enrich_turn_pairs import additional_pairs, freeze
from test_turns import thread
from turn_ancestry import CapturedThread
from turn_study_corpus import Cohort, Entry, Phase
from turns import extract


class EnrichmentTests(unittest.TestCase):
    def test_sampler_is_text_free_deterministic_and_respects_exclusions(self):
        identities = tuple(str(i) for i in range(8))
        excluded = frozenset((frozenset(("0", "2")), frozenset(("1", "5"))))
        pairs = additional_pairs(identities, excluded)
        self.assertEqual(pairs, additional_pairs(identities, excluded))
        self.assertEqual(len(pairs), 4)
        self.assertTrue(all(b - a >= 2 for a, b in pairs))
        self.assertTrue(
            all(
                frozenset((identities[a], identities[b])) not in excluded
                for a, b in pairs
            )
        )
        self.assertEqual(
            additional_pairs(("a", "b"), frozenset((frozenset(("a", "b")),))), ()
        )

    def test_freeze_preserves_initial_manifest_and_refuses_second_batch(self):
        previous_umask = os.umask(0o077)
        try:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                source = thread((Role.USER, Role.ASSISTANT) * 5)
                turns = extract(source)
                captured = CapturedThread(
                    index=0,
                    thread=source,
                    canonical=(),
                    resolutions=(),
                    activities=(),
                    original_turns=turns,
                    bridged_turns=turns,
                    turns=turns,
                    source_sha256="source",
                    native_matched=0,
                    native_parent_checked=0,
                    native_parent_mismatches=0,
                    native_compactions=0,
                )
                cohort = Cohort(
                    previous_sha256=(),
                    gold_manifest_sha256="gold",
                    histories_considered=1,
                    native_lineage_files_checked=0,
                    rejected={},
                    entries=(
                        Entry(
                            phase=Phase.DEVELOPMENT,
                            component="component",
                            previously_used=False,
                            captured=captured,
                        ),
                    ),
                )
                cohort_path = root / "cohort.json"
                cohort_path.write_text(cohort.model_dump_json())
                protocol = root / "protocol.md"
                protocol.write_text("synthetic frozen protocol")
                output = root / "blind"
                initial = initial_freeze(cohort_path, protocol, output)
                original = (output / "manifest.json").read_bytes()
                enlarged = freeze(cohort_path, protocol, output)
                self.assertEqual(
                    (output / "manifest-initial.json").read_bytes(), original
                )
                self.assertEqual(
                    enlarged.initial_manifest_sha256,
                    hashlib.sha256(original).hexdigest(),
                )
                self.assertEqual(len(initial.references), 6)
                self.assertEqual(len(enlarged.references), 10)
                self.assertEqual(
                    len(
                        {
                            frozenset((r.a_turn_id, r.b_turn_id))
                            for r in enlarged.references
                        }
                    ),
                    10,
                )
                self.assertEqual(enlarged.references[:6], initial.references)
                for path in enlarged.packets:
                    packet = Packet.model_validate_json(path.read_bytes())
                    self.assertTrue(
                        all(
                            pair.a.full_text.exists() and pair.b.full_text.exists()
                            for pair in packet.pairs
                        )
                    )
                with self.assertRaisesRegex(ValueError, "already been frozen"):
                    freeze(cohort_path, protocol, output)
        finally:
            os.umask(previous_umask)


if __name__ == "__main__":
    unittest.main()
