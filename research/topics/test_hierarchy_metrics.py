import unittest

import numpy as np

from hierarchy_metrics import dendrogram_purity, triplet_signature
from online_tree import Mode, Tree


class HierarchyMetricTests(unittest.TestCase):
    def make_tree(self, order):
        vectors = {"a": [1., 0.], "b": [1., 0.], "c": [0., 1.], "d": [0., 1.]}
        tree = Tree(Mode.GRINCH)
        for identity in order:
            tree.insert(identity, np.asarray(vectors[identity]))
        tree.validate()
        return tree

    def test_known_purity_and_singleton_label_denominator(self):
        tree = self.make_tree("abcd")
        self.assertEqual(dendrogram_purity(tree, {"a": 0, "b": 0, "c": 1, "d": 1}), 1.)
        self.assertEqual(dendrogram_purity(tree, {"a": 0, "b": 1, "c": 0, "d": 1}), .5)
        self.assertIsNone(dendrogram_purity(tree, {"a": 0, "b": 1, "c": 2, "d": 3}))
        with self.assertRaisesRegex(ValueError, "every leaf"):
            dendrogram_purity(tree, {"a": 0})

    def test_triplet_signature_uses_canonical_identity_order(self):
        forward = triplet_signature(self.make_tree("abcd"))
        reverse = triplet_signature(self.make_tree("dcba"))
        self.assertEqual(forward, reverse)
        self.assertEqual(len(forward["triplets"]), 4)
        self.assertEqual(forward["closest_pairs"], [(0, 1), (0, 1), (2, 3), (2, 3)])
