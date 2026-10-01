import unittest

import numpy as np

from question_calibration import choose, fuse, passes


class QuestionCalibrationTests(unittest.TestCase):
    def metric(self, tp=14, fp=6, recall=.5):
        return {'tp': tp, 'fp': fp, 'precision': tp / (tp + fp) if tp + fp else None, 'recall': recall}

    def candidate(self, forward, reverse=None):
        return {'candidate': {'view': 'title', 'linkage': 'average'},
                'orders': {'forward': {'metrics': forward}, 'reverse': {'metrics': reverse or forward}}}

    def test_undefined_precision_and_too_few_links_are_not_eligible(self):
        self.assertFalse(passes(self.metric(0, 0)))
        self.assertFalse(passes(self.metric(19, 0)))
        self.assertTrue(passes(self.metric()))
        self.assertFalse(passes(self.metric(13, 7)))

    def test_all_arrival_orders_must_pass_without_relaxing_target(self):
        results = {'a': self.candidate(self.metric(), self.metric(13, 7))}
        self.assertIsNone(choose(results, 'title', 'average'))
        self.assertEqual(choose(results, 'title', 'average', robust=False), 'a')

    def test_selection_uses_worst_recall_then_precision_then_lexical_ties(self):
        results = {'a': self.candidate(self.metric(15, 5, .6), self.metric(15, 5, .3)),
                   'c': self.candidate(self.metric(16, 4, .4)), 'b': self.candidate(self.metric(16, 4, .4))}
        self.assertEqual(choose(results, 'title', 'average'), 'b')
        self.assertIsNone(choose(results, 'other', 'average'))

    def test_concatenation_matches_weighted_cosine_without_renormalizing_components(self):
        dense = np.array([[1., 0.], [0., 1.]])
        lexical = np.array([[.6, .8], [.8, .6]])
        joined = fuse(dense, lexical)
        np.testing.assert_allclose(joined @ joined.T, .75 * (dense @ dense.T) + .25 * (lexical @ lexical.T), atol=1e-15)
        with self.assertRaises(ValueError):
            fuse(dense, lexical[:1])


if __name__ == '__main__':
    unittest.main()
