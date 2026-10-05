"""Confirmation cannot open without a frozen choice or after annotation mutation."""

import hashlib
import tempfile
import unittest
from pathlib import Path

from blind_turn_pairs import (
    Annotations,
    Label,
    Manifest,
    Packet,
    Pair,
    Reference,
    Stratum,
    Verdict,
    View,
)
from confirm_turn_study import accepted_choice, unseal
from run_study_development import Report
from study_annotations import Agreement
from study_scoring import Decision, Summary
from turn_evaluation import Counts
from turn_study_corpus import Phase


def report(decision: Decision, manifest: bytes, labels: bytes) -> Report:
    return Report(
        cohort_sha256="cohort",
        protocol_sha256="protocol",
        annotation_manifest_sha256=hashlib.sha256(manifest).hexdigest(),
        annotation_sha256={
            "labels-packet-000.json": hashlib.sha256(labels).hexdigest()
        },
        artifact_sha256={},
        encoder_manifest_sha256="encoder",
        agreement=Agreement(
            pairs=0, agrees=0, decisive_pairs=0, decisive_agrees=0, conflicts=()
        ),
        candidates=(),
        baselines=(),
        decision=decision,
        selected_interval=None,
        rows=(),
    )


class ConfirmationTests(unittest.TestCase):
    def test_only_frozen_baseline_can_be_fallback_and_must_pass_both_phases(self):
        good = Counts(tp=90, fp=5, fn=10, tn=200, ambiguous=0)
        weak = Counts(tp=50, fp=30, fn=50, tn=200, ambiguous=0)
        selected = Summary(name="complex", counts=good, seconds=1)
        baseline = Summary(name="simple", counts=good, seconds=1)
        self.assertEqual(accepted_choice(selected, baseline, True, good), "simple")
        self.assertIsNone(accepted_choice(selected, baseline, True, weak))
        self.assertEqual(accepted_choice(selected, baseline, False, good), "complex")

    def test_no_selection_rejects_before_filesystem_read(self):
        frozen = report(
            Decision(baseline="simple", selected=None, requires_gain=False), b"", b""
        )
        with self.assertRaisesRegex(ValueError, "confirmation stays closed"):
            unseal(Path("/must-not-read"), frozen)

    def test_matching_frozen_bytes_unseal_only_confirmation_and_changes_fail_closed(
        self,
    ):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            text = root / "turn.txt"
            text.write_text("database indexing latency")
            view = View(
                id="opaque", excerpt="database indexing latency", full_text=text
            )
            packet_path = root / "packet-000.json"
            packet_path.write_text(
                Packet(
                    pairs=tuple(Pair(id=i, a=view, b=view) for i in ("dev", "conf"))
                ).model_dump_json()
            )
            manifest = Manifest(
                cohort_sha256="cohort",
                protocol_sha256="protocol",
                packets=(packet_path,),
                review_packets=(),
                references=tuple(
                    Reference(
                        pair_id=i,
                        thread_index=n,
                        phase=phase,
                        a_turn_id="a",
                        b_turn_id="b",
                        stratum=Stratum.ADJACENT,
                    )
                    for n, (i, phase) in enumerate(
                        (("dev", Phase.DEVELOPMENT), ("conf", Phase.CONFIRMATION))
                    )
                ),
            )
            manifest_bytes = manifest.model_dump_json().encode()
            (root / "manifest.json").write_bytes(manifest_bytes)
            annotations = Annotations(
                annotator="blind",
                labels=tuple(
                    Label(
                        pair_id=i,
                        verdict=Verdict.RELATED,
                        subject="indexing",
                        note="Concrete indexing subject.",
                        evidence_a="database",
                        evidence_b="indexing",
                    )
                    for i in ("dev", "conf")
                ),
            )
            label_bytes = annotations.model_dump_json().encode()
            label_path = root / "labels-packet-000.json"
            label_path.write_bytes(label_bytes)
            frozen = report(
                Decision(baseline="simple", selected="simple", requires_gain=False),
                manifest_bytes,
                label_bytes,
            )
            _, labels = unseal(root, frozen)
            self.assertEqual(set(labels), {"conf"})
            label_path.write_bytes(b"malformed changed labels")
            with self.assertRaisesRegex(ValueError, "annotation bytes changed"):
                unseal(root, frozen)
            label_path.write_bytes(label_bytes)
            (root / "manifest.json").write_bytes(b"changed manifest")
            with self.assertRaisesRegex(ValueError, "manifest changed"):
                unseal(root, frozen)


if __name__ == "__main__":
    unittest.main()
