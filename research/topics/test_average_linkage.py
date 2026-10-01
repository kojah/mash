import unittest

import numpy as np

from external_hierarchy import fit_tree, topology
from online_tree import Linkage, Mode, Tree


class AverageLinkageTests(unittest.TestCase):
    def test_mean_dot_is_average_leaf_cosine_without_renormalization(self):
        tree = Tree(Mode.GRINCH, linkage=Linkage.AVERAGE)
        a = tree._node(np.array([1., 1.]), 2)
        b = tree._node(np.array([1., 0.]), 1)
        self.assertEqual(tree.score(a, b), .5)
        centroid = Tree(Mode.GRINCH)
        self.assertAlmostEqual(centroid.score(centroid._node(a.total, 2), centroid._node(b.total, 1)), 1 / np.sqrt(2))

    def test_generated_insertions_preserve_aggregates_and_algebra(self):
        rng = np.random.default_rng(20261001)
        raw = rng.normal(size=(8, 6))
        raw = raw / np.linalg.norm(raw, axis=1)[:, None]
        for mode in Mode:
            tree = Tree(mode, linkage=Linkage.AVERAGE)
            for index in rng.permutation(len(raw)):
                tree.insert(str(index), raw[index])
                tree.validate()
            nodes = list(tree.nodes.values())
            for a in nodes:
                for b in nodes:
                    left = tree.matrix[list(tree.leaf_indices(a))]
                    right = tree.matrix[list(tree.leaf_indices(b))]
                    self.assertAlmostEqual(tree.score(a, b), np.clip((left @ right.T).mean(), -1, 1), places=12)

    def test_canceling_sums_and_default_centroid_are_preserved(self):
        for linkage in Linkage:
            tree = Tree(Mode.GRINCH, linkage=linkage)
            tree.insert('a', np.array([1., 0.]))
            tree.insert('b', np.array([-1., 0.]))
            tree.validate()
            np.testing.assert_array_equal(tree.root.link_vector, [0., 0.])
        ids = ['a', 'b', 'c', 'd']
        matrix = np.array([[1., 0], [1., 0], [0., 1], [0., 1]])
        default = topology(fit_tree(ids, matrix, list(range(4)), Mode.GRINCH)[0], [])
        explicit = topology(fit_tree(ids, matrix, list(range(4)), Mode.GRINCH, Linkage.CENTROID)[0], [])
        self.assertEqual(default, explicit)
        with self.assertRaises(ValueError):
            Tree(Mode.GRINCH, linkage='unknown')


if __name__ == '__main__':
    unittest.main()
