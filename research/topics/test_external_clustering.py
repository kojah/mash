import unittest

import numpy as np

from external_clustering import fit, memberships, pair_metrics, representations
from online_tree import Mode, Tree


class ClusteringTests(unittest.TestCase):
    def test_dimension_limit_requires_explicit_opt_in(self):
        vector = np.zeros(4167)
        vector[0] = 1
        with self.assertRaises(ValueError):
            Tree(Mode.GRINCH).insert('a', vector)
        tree = Tree(Mode.GRINCH, maximum_leaves=1, maximum_dimensions=8192)
        tree.insert('a', vector)
        tree.validate()
        for limit in (0, 8193, True):
            with self.assertRaises(ValueError):
                Tree(Mode.GRINCH, maximum_dimensions=limit)

    def test_pair_confusion_and_undefined_precision(self):
        assignment = {'a': 0, 'b': 0, 'c': 1, 'd': 1}
        rows = [{'left': a, 'right': b, 'truth': t} for a, b, t in
                [('a', 'b', True), ('c', 'd', False), ('a', 'c', True), ('b', 'd', False)]]
        self.assertEqual(pair_metrics(rows, assignment, 'truth'),
                         {'tp': 1, 'fp': 1, 'fn': 1, 'tn': 1, 'pairs': 4, 'precision': .5, 'recall': .5, 'f1': .5})
        singleton = pair_metrics(rows, dict(zip('abcd', range(4))), 'truth')
        self.assertIsNone(singleton['precision'])
        self.assertEqual(singleton['recall'], 0)

    def test_partition_rejects_missing_duplicate_or_empty(self):
        for partition in ([['a']], [['a', 'b'], ['a']], [['a', 'b'], []]):
            with self.assertRaises(ValueError):
                memberships(partition, ['a', 'b'])
        self.assertEqual(memberships([['b'], ['a']], ['a', 'b']), {'a': 0, 'b': 1})

    def test_concat_matches_arithmetic_cosine(self):
        lexical = {'vocabulary': ['x', 'y'], 'records': {
            'a': {'reason': None, 'indices': [0], 'values': [1.]},
            'b': {'reason': None, 'indices': [1], 'values': [1.]}}}
        vectors = {'current': {'a': np.array([1., 0]), 'b': np.array([.6, .8])},
                   'joined': {'a': np.array([0., 1]), 'b': np.array([.8, .6])}}
        result = representations(['a', 'b'], vectors, lexical)
        self.assertAlmostEqual(result['current_75_lexical_25'][0] @ result['current_75_lexical_25'][1], .75 * .6)
        lexical['records']['a']['indices'] = [2]
        with self.assertRaises(ValueError):
            representations(['a', 'b'], vectors, lexical)

    def test_streamed_separable_groups_and_permutation_guard(self):
        identities = ['a', 'b', 'c', 'd']
        matrix = np.array([[1., 0], [1., 0], [0., 1], [0., 1]])
        for method in ('birch', 'grinch'):
            for order in ([0, 1, 2, 3], [3, 2, 1, 0]):
                result, _ = fit(identities, matrix, order, method)
                self.assertEqual(result['clusters'], [['a', 'b'], ['c', 'd']])
            with self.assertRaises(ValueError):
                fit(identities, matrix, [0, 0, 2, 3], method)


if __name__ == '__main__':
    unittest.main()
