from dataclasses import replace
import json
from pathlib import Path
import tempfile
import unittest

import numpy as np

from episodes import Case, Judgment, Turn
from lexical_boundaries import Features, Method, admissions, boundary_score, metrics, sha


class LexicalTests(unittest.TestCase):
    def case(self):
        x, y = np.eye(2)
        return Case("case", "codex", "source", tuple(Turn(str(i), i, str(i), v) for i, v in enumerate((x, x, y, y))), ())

    def features(self, case):
        return Features({t.id: t.vector for t in case.turns}, frozenset(t.id for t in case.turns))

    def test_disjoint_blocks_do_not_borrow_unbounded_future_and_barriers_abstain(self):
        case = self.case()
        features = self.features(case)
        self.assertEqual(boundary_score(case, 2, Method.LEXICAL_BLOCKS, features), 1)
        barrier = replace(features, admitted=features.admitted - {"1"})
        self.assertIsNone(boundary_score(case, 2, Method.DENSE_BLOCKS, barrier))
        self.assertIsNone(boundary_score(case, 2, Method.LEXICAL_BLOCKS, barrier))
        for method in Method:
            self.assertEqual(boundary_score(case, 2, method, features), 1)
        extended = replace(case, turns=(*case.turns, Turn("4", 4, "future", np.ones(2))))
        edited = replace(extended, turns=(*extended.turns[:4], replace(extended.turns[4], vector=-np.ones(2))))
        self.assertEqual(boundary_score(extended, 2, Method.DENSE_BLOCKS, features),
                         boundary_score(edited, 2, Method.DENSE_BLOCKS, features))

    def test_missing_or_cancelling_vectors_abstain_without_hybrid_fallback(self):
        case = self.case()
        features = self.features(case)
        missing = replace(case, turns=(case.turns[0], replace(case.turns[1], vector=None), *case.turns[2:]))
        self.assertIsNone(boundary_score(missing, 2, Method.HYBRID, features))
        cancelling = replace(case, turns=(case.turns[0], replace(case.turns[1], vector=-case.turns[0].vector), *case.turns[2:]))
        self.assertIsNone(boundary_score(cancelling, 2, Method.DENSE_BLOCKS, features))
        zero = replace(features, lexical={**features.lexical, "1": np.zeros(2)})
        self.assertIsNone(boundary_score(case, 2, Method.LEXICAL, zero))

    def test_tied_scores_no_change_signal_and_uncertainty_excluded(self):
        rows = [{"judgment": Judgment.SAME, "score": 1}, {"judgment": Judgment.CHANGE, "score": 1},
                {"judgment": Judgment.UNCERTAIN, "score": 0}, {"judgment": Judgment.CHANGE, "score": None}]
        result = metrics(rows)
        self.assertEqual((result["known"], result["scored"], result["abstained"]), (3, 2, 1))
        self.assertEqual(result["average_precision"], 0.5)
        self.assertEqual(result["roc_auc"], 0.5)

    def test_source_admission_is_complete_identity_pinned_and_excludes_secrets(self):
        case = self.case()
        sidecar = {"schema_version": 1, "cases": [{"thread": case.thread, "source_sha256": case.source_sha256,
                   "admission_judgments": [{"message": t.id, "ordinal": t.ordinal,
                    "text_sha256": sha(t.text.encode()), "kind": "sensitive" if t.id == "0" else "content"}
                    for t in case.turns]}]}
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "review.json"
            path.write_text(json.dumps(sidecar))
            self.assertNotIn("0", admissions((case,), path))
            sidecar["cases"][0]["admission_judgments"][1]["text_sha256"] = "edited"
            path.write_text(json.dumps(sidecar))
            with self.assertRaisesRegex(ValueError, "text changed"):
                admissions((case,), path)
