import json
import unittest

import numpy as np

from external_hierarchy import fit_tree, metrics, stability, topology
from online_tree import Mode


class HierarchyTests(unittest.TestCase):
    def setUp(self):
        self.identities = ['a', 'b', 'c', 'd']
        self.matrix = np.array([[1., 0], [1., 0], [0., 1], [0., 1]])
        self.rows = [{'left': a, 'right': b, 'service_overlap': gold, 'intent_overlap': gold} for a, b, gold in
                     [('a', 'b', True), ('c', 'd', True), ('a', 'c', False), ('a', 'd', False), ('b', 'c', False), ('b', 'd', False)]]

    def test_lca_counts_and_complete_canonical_clades(self):
        tree, _ = fit_tree(self.identities, self.matrix, [0, 1, 2, 3], Mode.GRINCH)
        result = topology(tree, self.rows)
        self.assertEqual([p['leaves'] for p in result['pairs']], [2, 2, 4, 4, 4, 4])
        for p in result['pairs']:
            self.assertEqual(len(result['clades'][p['lca']]), p['leaves'])
            self.assertIn(p['left'], result['clades'][p['lca']])
            self.assertIn(p['right'], result['clades'][p['lca']])
        self.assertEqual(sorted(map(len, result['clades'].values())), [2, 2, 4])
        self.assertEqual(metrics(self.rows, [p['score'] for p in result['pairs']])['intent']['metrics'], {'ap': 1., 'auc': 1.})

    def test_topology_round_trip_and_id_order_stability(self):
        a = topology(fit_tree(self.identities, self.matrix, [0, 1, 2, 3], Mode.GRINCH)[0], self.rows)
        b = topology(fit_tree(self.identities, self.matrix, [3, 2, 1, 0], Mode.GRINCH)[0], self.rows)
        self.assertEqual(json.loads(json.dumps(a)), a)
        self.assertEqual(stability(a, b), {'triplets': 4, 'triplet_agreement': 1., 'clade_jaccard': 1.})
        b['signature']['triplets'].pop()
        with self.assertRaises(ValueError):
            stability(a, b)

    def test_inactive_leaf_still_counts_in_LCA_mass(self):
        tree, _ = fit_tree(self.identities, self.matrix, [0, 1, 2, 3], Mode.GREEDY)
        # Only this pair has evaluation labels. Other fit leaves are not erased.
        result = topology(tree, [self.rows[2]])
        self.assertEqual(result['pairs'][0]['leaves'], 3)
        self.assertIn('d', result['clades'][result['pairs'][0]['lca']])

    def test_invalid_fit_or_missing_scores_are_rejected(self):
        for matrix, order in ((np.zeros((4, 2)), [0, 1, 2, 3]), (self.matrix, [0, 0, 2, 3]),
                              (np.array([[float('nan'), 0]] * 4), [0, 1, 2, 3])):
            with self.assertRaises(ValueError):
                fit_tree(self.identities, matrix, order, Mode.GRINCH)
        with self.assertRaises(ValueError):
            metrics(self.rows, [])


if __name__ == '__main__':
    unittest.main()
