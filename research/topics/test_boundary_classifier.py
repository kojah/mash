import unittest

import numpy as np

from boundary_classifier import features, fit, oof


class BoundaryClassifierTests(unittest.TestCase):
    def test_overlap_repetition_unicode_and_only_past_context(self):
        text = ['Café café alpha', 'ALPHA beta', 'café beta beta', 'future only']
        x = features(text, np.zeros((3, 2)))
        self.assertEqual(x[0, 2], 1/3)
        self.assertEqual(x[1, 2], 1/3)
        self.assertEqual(x[1, 3], 2/3)
        self.assertEqual(x[1, 4], np.log1p(3))
        self.assertTrue(np.array_equal(x[:2], features(text[:3], np.zeros((2, 2)))))

    def test_empty_tokens_do_not_make_artificial_cohesion(self):
        x = features(['! a ?', '?!'], np.zeros((1, 2)))
        self.assertTrue(np.array_equal(x, np.zeros((1, 6))))

    def test_source_and_geometry_fail_closed(self):
        for texts, scores in [(['only'], np.zeros((0, 2))), (['a', ' '], np.zeros((1, 2))),
                              (['a', 'b'], [[np.nan, 0]]), (['a', 'b'], [[2, 0]]),
                              (['a', 'b'], np.zeros((2, 2)))]:
            with self.assertRaises(ValueError):features(texts, scores)

    def test_fold_scaler_has_no_held_dialogue_rows(self):
        x = np.arange(120, dtype=float).reshape(20, 6)
        y = np.tile([False, True], 10);groups = np.repeat(np.arange(10).astype(str), 2)
        p, assignment, folds = oof(x, y, groups)
        self.assertTrue(np.isfinite(p).all())
        self.assertEqual(set(assignment), set(range(5)))
        for fold in folds:
            held = assignment == fold['fold'];train = ~held
            self.assertFalse(set(fold['training_groups']) & set(fold['held_groups']))
            self.assertEqual(fold['model']['mean'], x[train].mean(axis=0).tolist())
            self.assertEqual(fold['model']['training_rows'], int(train.sum()))

    def test_fit_requires_both_classes_and_constant_features_have_unit_scale(self):
        x = np.zeros((10, 6));y = np.tile([False, True], 5)
        _, _, state = fit(x, y)
        self.assertEqual(state['scale'], [1.]*6)
        with self.assertRaises(ValueError):fit(x, np.zeros(10, dtype=bool))
        with self.assertRaises(ValueError):oof(x, y, np.zeros(10, dtype=int))


if __name__ == '__main__':unittest.main()
