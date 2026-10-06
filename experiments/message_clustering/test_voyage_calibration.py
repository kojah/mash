"""Boundary and differential tests independent of external inference or labels."""

import unittest

import numpy as np
import optuna

from ablation_evidence import fit, fuse
from clustering import normalized_vectors
from run_voyage_calibration import (
    Geometry,
    Parameters,
    connections,
    geometry,
    remaining_trials,
    split_indices,
)
from voyage_vectors import (
    Item,
    Model,
    Response,
    Usage,
    batches,
    codec,
    decode,
    structural_parts,
)


class CalibrationTests(unittest.TestCase):
    def test_queued_baseline_does_not_consume_trial_budget(self) -> None:
        study = optuna.create_study()
        study.enqueue_trial({"radius": 0.60})
        self.assertEqual(remaining_trials(study), 48)
        study.optimize(lambda _: 0.0, n_trials=1)
        self.assertEqual(remaining_trials(study), 47)
        study.enqueue_trial({"radius": 0.50})
        self.assertEqual(remaining_trials(study), 47)

    def test_registered_memberships_match_original_fusion(self) -> None:
        rng = np.random.default_rng(4)
        points = normalized_vectors(rng.normal(size=(30, 8)))
        owners = tuple(str(i // 3) for i in range(len(points)))
        pooled = normalized_vectors(points.reshape(10, 3, 8).mean(axis=1))
        pooled_owners = tuple(str(i) for i in range(10))
        expected = fuse(
            fit(pooled, pooled_owners), fit(points, owners), (0.0,) * 30, False
        )
        a = connections(geometry(pooled, pooled_owners, 0.60), Parameters())
        b = connections(geometry(points, owners, 0.60), Parameters())
        for first in pooled_owners:
            for second in pooled_owners:
                self.assertEqual(
                    bool(expected[first] & expected[second]),
                    bool(a[first] & a[second]) or bool(b[first] & b[second]),
                )

    def test_relative_gate_and_distinct_owner_support(self) -> None:
        value = Geometry(
            ("a", "b"),
            ("a", "a", "b"),
            np.array([[0.90, 0.76], [0.89, 0.76], [0.76, 0.90]]),
        )
        narrow = connections(value, Parameters())
        self.assertFalse(narrow["a"] & narrow["b"])
        broad = connections(value, Parameters(margin=1.0))
        self.assertTrue(broad["a"] & broad["b"])

    def test_thread_split_is_order_independent_and_disjoint(self) -> None:
        ids = tuple(range(156))
        a, b = split_indices(ids)
        self.assertEqual((a, b), split_indices(tuple(reversed(ids))))
        self.assertEqual(len(a), 104)
        self.assertEqual(len(b), 52)
        self.assertFalse(set(a) & set(b))
        self.assertEqual(set(a) | set(b), set(ids))

    def test_structural_split_is_utf8_bounded_and_lossless(self) -> None:
        text = "User:\n東京" * 19 + "\n\nAssistant:\n" + "α" * 100
        parts = structural_parts(text, 32)
        self.assertEqual("".join(parts), text)
        self.assertTrue(all(0 < len(p.encode()) <= 32 for p in parts))
        self.assertEqual(structural_parts("small turn"), ("small turn",))

    def test_response_indices_and_raw_floats_are_preserved(self) -> None:
        first = (0.123456789,) + (0.0,) * 1023
        second = (0.0, 1.0) + (0.0,) * 1022
        response = Response(
            model="voyage-4",
            data=(Item(index=1, embedding=second), Item(index=0, embedding=first)),
            usage=Usage(total_tokens=3),
        )
        raw = decode(response, 2, Model.GENERAL)
        self.assertEqual(raw[0, 0], first[0])
        self.assertEqual(raw[1, 1], 1.0)
        self.assertEqual(codec(raw).shape, raw.shape)
        np.testing.assert_allclose(np.linalg.norm(codec(raw), axis=1), 1.0)
        duplicate = response.model_copy(
            update={"data": (response.data[0], response.data[0])}
        )
        with self.assertRaisesRegex(ValueError, "index"):
            decode(duplicate, 2, Model.GENERAL)
        with self.assertRaisesRegex(ValueError, "model"):
            decode(response, 2, Model.CODE)

    def test_batching_enforces_per_input_and_total_limits(self) -> None:
        work = batches(tuple("a" * 27000 for _ in range(8)))
        self.assertEqual(tuple(map(len, work)), (3, 3, 2))
        with self.assertRaises(ValueError):
            batches(("a" * 28001,))

    def test_invalid_response_vectors_are_rejected(self) -> None:
        for embedding in ((0.0,) * 1024, (float("nan"),) + (0.0,) * 1023, (1.0,) * 384):
            response = Response(
                model="voyage-code-4",
                data=(Item(index=0, embedding=embedding),),
                usage=Usage(total_tokens=1),
            )
            with self.assertRaises(ValueError):
                decode(response, 1, Model.CODE)


if __name__ == "__main__":
    unittest.main()
