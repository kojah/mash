"""Selection must enforce all three targets without evaluation-label access."""

import unittest

import numpy as np

from run_high_recall import (
    Candidate,
    Family,
    Quality,
    alternative_geometry,
    blend,
    choose,
)
from run_voyage_calibration import Parameters
from turn_evaluation import Counts


class HighRecallTests(unittest.TestCase):
    def test_alternative_families_keep_supported_groups_only(self):
        vectors = np.array([[1.0, 0.0], [1.0, 0.0], [0.0, 1.0]])
        for family in (Family.AVERAGE, Family.COMPLETE, Family.LEADER):
            with self.subTest(family=family):
                value = alternative_geometry(vectors, ("a", "b", "c"), 0.7, family)
                self.assertEqual(value.scores.shape, (3, 1))
                np.testing.assert_allclose(value.scores[:, 0], [1.0, 1.0, 0.0])

    def test_ensemble_cosine_is_convex_and_rows_must_align(self):
        code = np.array([[1.0, 0.0], [0.0, 1.0]])
        general = np.array([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
        vectors = blend(code, general, 0.75)
        self.assertAlmostEqual(float(vectors[0] @ vectors[1]), 0.25)
        np.testing.assert_allclose(np.linalg.norm(vectors, axis=1), 1.0)
        with self.assertRaises(ValueError):
            blend(code, general[:1], 0.5)

    def test_targets_are_strict_and_all_required(self):
        exact = Quality(
            counts=Counts(tp=82, fp=18, fn=8, tn=10, ambiguous=0),
            distant_related=10,
            distant_missed=2,
        )
        self.assertFalse(exact.feasible)
        good = Quality(
            counts=Counts(tp=92, fp=8, fn=8, tn=10, ambiguous=0),
            distant_related=10,
            distant_missed=1,
        )
        self.assertTrue(good.feasible)
        self.assertGreater(good.objective, exact.objective)

    def test_feasible_choice_beats_high_precision_with_insufficient_recall(self):
        good = Candidate(
            number=1,
            parameters=Parameters(),
            calibration=Quality(
                counts=Counts(tp=92, fp=8, fn=8, tn=10, ambiguous=0),
                distant_related=10,
                distant_missed=1,
            ),
        )
        weak = Candidate(
            number=0,
            parameters=Parameters(),
            calibration=Quality(
                counts=Counts(tp=80, fp=0, fn=20, tn=10, ambiguous=0),
                distant_related=10,
                distant_missed=3,
            ),
        )
        self.assertEqual(choose((weak, good)), good)


if __name__ == "__main__":
    unittest.main()
