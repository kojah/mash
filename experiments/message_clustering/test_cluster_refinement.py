"""Refinement removes weak links while preserving overlapping group identities."""

import unittest

import numpy as np

from run_cluster_refinement import refine


class RefinementTests(unittest.TestCase):
    def test_refinement_preserves_overlap_and_discards_weak_singletons(self):
        vectors = np.array([[1.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, 1.0]])
        owners = ("a", "b", "c", "d")
        groups = {
            "a": frozenset({0, 1}),
            "b": frozenset({0, 1}),
            "c": frozenset({0}),
            "d": frozenset({0}),
        }
        result = refine(groups, vectors, owners, 0.7)
        self.assertEqual(len(result["a"]), 2)
        self.assertTrue(result["a"] & result["b"])
        self.assertTrue(result["c"] & result["d"])
        self.assertFalse(result["a"] & result["c"])
        self.assertEqual(refine(groups, vectors, owners, 0), groups)


if __name__ == "__main__":
    unittest.main()
