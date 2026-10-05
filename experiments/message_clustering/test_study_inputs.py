"""Guard blind evidence boundaries and equal-weight turn view aggregation."""

import tempfile
import unittest
from pathlib import Path

import numpy as np

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
    sample,
    validate,
)
from corpus import Role
from study_annotations import load_development
from study_artifacts import separate_pool, separate_units
from test_turns import thread, tokenizer
from turn_study_corpus import Phase
from turns import Unit, extract


class StudyInputs(unittest.TestCase):
    def test_development_boundary_hides_confirmation_and_keeps_primary_labels(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            full = root / "text.txt"
            full.write_text("database indexing")
            view = View(id="opaque", excerpt="database indexing", full_text=full)
            packet = Packet(
                pairs=tuple(Pair(id=i, a=view, b=view) for i in ("dev", "sealed"))
            )
            path = root / "packet-000.json"
            review = root / "review-000.json"
            path.write_text(packet.model_dump_json())
            review.write_text(packet.model_dump_json())
            primary = Annotations(
                annotator="a",
                labels=tuple(
                    Label(
                        pair_id=i,
                        verdict=Verdict.RELATED,
                        subject="indexing",
                        note="Shared database indexing.",
                        evidence_a="database",
                        evidence_b="indexing",
                    )
                    for i in ("dev", "sealed")
                ),
            )
            secondary = Annotations(
                annotator="b",
                labels=tuple(
                    label.model_copy(
                        update={"verdict": Verdict.UNRELATED, "subject": None}
                    )
                    for label in primary.labels
                ),
            )
            (root / "labels-packet-000.json").write_text(primary.model_dump_json())
            (root / "labels-review-000.json").write_text(secondary.model_dump_json())
            manifest = Manifest(
                cohort_sha256="cohort",
                protocol_sha256="protocol",
                packets=(path,),
                review_packets=(review,),
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
                        (("dev", Phase.DEVELOPMENT), ("sealed", Phase.CONFIRMATION))
                    )
                ),
            )
            (root / "manifest.json").write_text(manifest.model_dump_json())
            data = load_development(root)
            self.assertEqual(set(data.labels), {"dev"})
            self.assertEqual(data.labels["dev"].verdict, Verdict.RELATED)
            self.assertEqual(data.agreement.pairs, 1)
            self.assertEqual(data.agreement.conflicts, ("dev",))
            self.assertFalse(data.confirmation_adequate)

    def test_sampler_is_deterministic_distinct_and_handles_empty_vocabulary(self):
        prompts = tuple(f"database latency issue {i}" for i in range(10))
        pairs = sample(prompts, "history")
        self.assertEqual(pairs, sample(prompts, "history"))
        self.assertEqual(len(pairs), 6)
        self.assertEqual(len({(a, b) for a, b, _ in pairs}), 6)
        self.assertTrue(all(0 <= a < b < len(prompts) for a, b, _ in pairs))
        self.assertEqual(len(sample(("the", "and"), "empty")), 1)
        self.assertEqual(len(sample(("one", "two", "three"), "short")), 3)

    def test_validator_rejects_fabricated_quotes_duplicate_ids_and_missing_subject(
        self,
    ):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "turn.txt"
            path.write_text("Database latency and indexing are concrete subjects.")
            view = View(id="opaque", excerpt="Database latency", full_text=path)
            packet = Packet(pairs=(Pair(id="pair", a=view, b=view),))
            label = Label(
                pair_id="pair",
                verdict=Verdict.RELATED,
                subject="database latency",
                note="Both concern database latency.",
                evidence_a="Database latency",
                evidence_b="indexing",
            )
            validate(packet, Annotations(annotator="blind", labels=(label,)))
            for labels in (
                (label, label),
                (),
                (label.model_copy(update={"subject": None}),),
                (label.model_copy(update={"evidence_b": "fabricated quote"}),),
            ):
                with self.assertRaises(ValueError):
                    validate(packet, Annotations(annotator="blind", labels=labels))

    def test_separate_pool_preserves_equal_view_weight_and_prompt_only_fallback(self):
        source = thread((Role.USER, Role.ASSISTANT, Role.USER))
        turns = extract(source)
        prepared = tuple(
            Unit(
                id=str(i),
                turn_id=turns[t].id,
                text="",
                spans=(),
                represented_sources=(s,),
                tokens=0,
            )
            for i, (t, s) in enumerate(((0, 0), (0, 1), (0, 1), (1, 2)))
        )
        vectors = np.array([[1.0, 0.0], [0.0, 1.0], [0.0, 1.0], [1.0, 0.0]])
        pooled = separate_pool(turns, prepared, vectors)
        np.testing.assert_allclose(pooled[0], np.array([1.0, 1.0]) / np.sqrt(2))
        np.testing.assert_allclose(pooled[1], [1.0, 0.0])

    def test_separate_windows_cover_answer_without_prompt_contamination(self):
        source = thread((Role.USER, Role.ASSISTANT), ("question", "word " * 1200))
        turn = extract(source)[0]
        prepared = separate_units(source, turn, tokenizer())
        self.assertTrue(all(u.tokens <= 512 for u in prepared))
        self.assertTrue(all(len(u.represented_sources) == 1 for u in prepared))
        covered = {
            i
            for u in prepared
            for span in u.spans
            if span.source == 1
            for i in range(span.start, span.end)
        }
        self.assertTrue(
            all(
                i in covered
                for i, c in enumerate(source.messages[1].projection)
                if not c.isspace()
            )
        )


if __name__ == "__main__":
    unittest.main()
