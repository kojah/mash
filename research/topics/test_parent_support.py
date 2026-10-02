import unittest

import numpy as np

from parent_support import Weight, replicated, supports
from word_graph import Graph


class ParentSupportTests(unittest.TestCase):
    def test_repetition_in_one_parent_adds_no_votes(self):
        records = [{'parent': 'a', 'words': [0, 1]}] * 100 + [{'parent': 'b', 'words': [0, 1, 2]}]
        counts, events = supports(records, ['a', 'b'], 3, [[0, 1], [0, 2], [1, 2]])
        np.testing.assert_array_equal(counts, [2, 1, 1])
        self.assertEqual(events, 103)

    def test_words_in_separate_windows_do_not_create_pair(self):
        counts, _ = supports([{'parent': 'a', 'words': [0]}, {'parent': 'a', 'words': [1]}], ['a'], 2, [[0, 1]])
        np.testing.assert_array_equal(counts, [0])

    def test_weight_controls_preserve_tests_and_require_replication(self):
        graph = Graph(np.array([2, 2, 2]), np.array([[0, 1], [0, 2], [1, 2]]),
                      np.array([20, 10, 5]), np.array([.01, .001, .1]), np.array([True, True, False]), .05)
        before = graph.counts.copy()
        support = np.array([2, 1, 3])
        capped = replicated(graph, support, Weight.PARENT)
        original = replicated(graph, support, Weight.WINDOW)
        np.testing.assert_array_equal(capped.selected, [True, False, False])
        np.testing.assert_array_equal(original.selected, capped.selected)
        np.testing.assert_array_equal(capped.counts, support)
        np.testing.assert_array_equal(original.counts, before)
        np.testing.assert_array_equal(graph.counts, before)
        np.testing.assert_array_equal(capped.probabilities, graph.probabilities)

    def test_invalid_parent_words_pairs_counts_and_caps_rejected(self):
        for record in ({'parent': 'unknown', 'words': [0]}, {'parent': 'a', 'words': [1, 0]},
                       {'parent': 'a', 'words': [0, 0]}, {'parent': 'a', 'words': [True]},
                       {'parent': 'a', 'words': [-1]}):
            with self.assertRaises(ValueError):
                supports([record], ['a'], 2, [[0, 1]])
        with self.assertRaises(ValueError):
            supports([], ['a'], 2, [[1, 0]])
        with self.assertRaises(ValueError):
            supports([{'parent': 'a', 'words': list(range(1024))}] * 4, ['a'], 1024, [[0, 1]])
        graph = Graph(np.array([1, 1]), np.array([[0, 1]]), np.array([2]), np.array([.01]), np.array([True]), .05)
        for support in ([-1], [3], [.5], []):
            with self.assertRaises(ValueError):
                replicated(graph, support, Weight.PARENT)


if __name__ == '__main__':
    unittest.main()
