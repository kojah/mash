import unittest

import numpy as np
from sklearn.decomposition import LatentDirichletAllocation

from online_lda import fit
from word_graph import assignments


class OnlineLDATests(unittest.TestCase):
    def test_repeat_normalization_and_empty_source_abstention(self):
        counts = [[3, 0, 1], [0, 0, 0], [0, 4, 1]]
        first, _ = fit(counts, [2, 1, 0], 2, 2)
        second, _ = fit(counts, [2, 1, 0], 2, 2)
        self.assertEqual(first, second)
        self.assertEqual(first['empty_sources'], [1])
        self.assertEqual(first['final_scores'][1], [0, 0])
        self.assertEqual(assignments(first['final_scores'], .1, 3)[1], [])
        np.testing.assert_allclose(np.asarray(first['phi']).sum(1), 1, atol=1e-12)
        self.assertEqual(first['updates'], 2)
        self.assertEqual(first['library_batch_counter'], 3)

    def test_batches_cover_source_once_per_epoch_and_capture_first_arrival(self):
        counts = np.tile([[3, 0, 1], [0, 4, 1]], (10, 1))
        order = list(reversed(range(20)))
        model, _ = fit(counts, order, 2, 2)
        self.assertEqual(model['batches'], [{'epoch': e, 'sources': order[s:s + 16]}
                                          for e in range(2) for s in (0, 16)])
        direct = LatentDirichletAllocation(n_components=2, doc_topic_prior=.5, topic_word_prior=.5,
            learning_method='online', learning_decay=.7, learning_offset=10, batch_size=16,
            total_samples=20, max_iter=1, evaluate_every=-1, mean_change_tol=.001,
            max_doc_update_iter=100, n_jobs=1, random_state=17)
        direct.partial_fit(counts[order[:16]])
        expected = direct.transform(counts[order[:16]])
        np.testing.assert_array_equal(np.asarray(model['first_arrival_scores'])[order[:16]], expected)
        for batch in (order[16:], order[:16], order[16:]):
            direct.partial_fit(counts[batch])
        np.testing.assert_array_equal(model['components'], direct.components_)
        np.testing.assert_array_equal(model['final_scores'], direct.transform(counts))

    def test_invalid_counts_rejected_without_truncation(self):
        for counts in ([[1.5, 2]], [[-1, 2]], [[np.nan, 2]], [[np.inf, 2]], [[0, 0]],
                       [], np.zeros((513, 2)), np.ones((1, 1025)), [[10_000_001, 0]]):
            with self.assertRaises(ValueError):
                fit(counts, [0], 2, 1)

    def test_malformed_orders_and_parameters_rejected(self):
        for order, topics, epochs in [([0, 0], 2, 1), ([0], 2, 1), ([0, 2], 2, 1),
                                     ([False, 1], 2, 1), (None, 2, 1), ([0, 1], True, 1),
                                     ([0, 1], 65, 1), ([0, 1], 2, 0), ([0, 1], 2, 21)]:
            with self.assertRaises(ValueError):
                fit([[1, 0], [0, 1]], order, topics, epochs)


if __name__ == '__main__':
    unittest.main()
