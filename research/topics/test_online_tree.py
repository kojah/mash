import unittest

import numpy as np

from online_tree import Mode, Tree


class OnlineTreeTests(unittest.TestCase):
    def test_every_insert_preserves_binary_tree_membership_and_aggregates(self):
        rng = np.random.default_rng(17)
        vectors = rng.normal(size=(32, 12))
        for mode in Mode:
            tree = Tree(mode, maximum_leaves=32)
            for i, vector in enumerate(vectors):
                tree.insert(str(i), vector)
                tree.validate()
            self.assertEqual(tree.root.count, 32)
            self.assertEqual(sorted(i for cluster in tree.cut(.7) for i in cluster), list(range(32)))
            self.assertEqual(tree.cut(-1), (tuple(range(32)),))

    def test_identical_and_canceling_vectors_remain_finite_and_preserve_all_leaves(self):
        for mode in Mode:
            tree = Tree(mode)
            for i, vector in enumerate([[1., 0.]] * 8 + [[-1., 0.]] * 8):
                tree.insert(str(i), np.asarray(vector))
                tree.validate()
            self.assertEqual(tree.root.count, 16)
            self.assertTrue(np.isfinite(tree.root.link_vector).all())

    def test_invalid_inputs_do_not_change_existing_tree(self):
        tree = Tree(Mode.GRINCH, maximum_leaves=2)
        tree.insert("a", np.asarray([1., 0.]))
        for identity, vector in [("a", [0., 1.]), ("", [0., 1.]), ("b", [0., 0.]),
                                 ("b", [np.nan, 1.]), ("b", [1., 0., 0.])]:
            with self.assertRaises(ValueError):
                tree.insert(identity, np.asarray(vector))
            tree.validate()
            self.assertEqual(tree.root.count, 1)
        tree.insert("b", np.asarray([0., 1.]))
        with self.assertRaisesRegex(ValueError, "bound"):
            tree.insert("c", np.asarray([1., 1.]))
        tree.validate()

    def test_leaf_vector_is_owned_not_aliased_to_caller(self):
        vector = np.asarray([1., 0.])
        tree = Tree(Mode.GRINCH)
        tree.insert("a", vector)
        vector[:] = 0
        tree.validate()
        np.testing.assert_array_equal(tree.root.total, [1., 0.])

    def test_validation_detects_stale_aggregates_and_broken_parent_links(self):
        tree = Tree(Mode.GRINCH)
        tree.insert("a", np.asarray([1., 0.]))
        tree.insert("b", np.asarray([0., 1.]))
        tree.root.count = 1
        with self.assertRaisesRegex(ValueError, "aggregate"):
            tree.validate()
        tree.root.count = 2
        tree.leaves[0].parent = None
        with self.assertRaisesRegex(ValueError, "parent"):
            tree.validate()

    def test_restructuring_recovers_similar_subtrees_without_losing_displaced_leaf(self):
        tree = Tree(Mode.GREEDY)
        for identity, vector in [("z", [1., 0.]), ("m", [1., .1]), ("other", [0., 1.])]:
            tree.insert(identity, np.asarray(vector))
        self.assertEqual(tree.lca(tree.leaves[0], tree.leaves[1]).count, 3)
        tree._restructure(tree.leaves[1], tree.root)
        tree.validate()
        self.assertEqual(tree.lca(tree.leaves[0], tree.leaves[1]).count, 2)
        self.assertEqual(tree.statistics.restructure_swaps, 1)
        self.assertEqual(tree.root.count, 3)

    def test_separated_classes_have_pure_subtrees_across_fixed_orders(self):
        vectors = []
        labels = []
        for label in range(4):
            for offset in range(4):
                vector = np.zeros(20)
                vector[label * 5 + offset:label * 5 + offset + 2] = 1
                vectors.append(vector)
                labels.append(label)
        for seed in range(5):
            tree = Tree(Mode.GRINCH)
            for i in np.random.default_rng(seed).permutation(len(vectors)):
                tree.insert(str(i), vectors[i])
                tree.validate()
            nodes = {identity: leaf for identity, leaf in zip(tree.identities, tree.leaves)}
            for label in range(4):
                members = [nodes[str(i)] for i, value in enumerate(labels) if value == label]
                ancestor = members[0]
                for member in members[1:]:
                    ancestor = tree.lca(ancestor, member)
                self.assertEqual({labels[int(tree.identities[i])] for i in tree.leaf_indices(ancestor)}, {label})
