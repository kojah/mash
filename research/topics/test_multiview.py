import unittest

import numpy as np

from external_analysis import summarize
from external_context import Control
from multiview import View, derived_vectors, represent


def basis(index):
    vector = np.zeros(384)
    vector[index] = 1.
    return vector


class MultiviewTests(unittest.TestCase):
    def test_concatenation_has_frozen_weighted_cosine_and_unit_norm(self):
        a, b = basis(0), basis(1)
        for view, weight in ((View.EQUAL, 0.5), (View.CURRENT_75, 0.75)):
            left = represent(a, b, 10, view)
            right = represent(a, -b, 11, view)
            self.assertEqual(left.shape, (768,))
            self.assertAlmostEqual(np.linalg.norm(left), 1.)
            self.assertAlmostEqual(left @ right, weight * (a @ a) + (1 - weight) * (b @ -b))
            # Opposing views do not cancel because their coordinates stay distinct.
            opposing = represent(a, -a, 10, view)
            self.assertAlmostEqual(np.linalg.norm(opposing), 1.)

    def test_short_gate_has_exact_boundary_and_no_phrase_features(self):
        current, joined = basis(0), basis(1)
        self.assertIs(represent(current, joined, 10, View.SHORT), joined)
        self.assertIs(represent(current, joined, 11, View.SHORT), current)
        self.assertIs(represent(current, joined, 2, View.SHORT), joined)

    def test_malformed_or_unbounded_features_are_rejected(self):
        for current, tokens, view in ((np.zeros(384), 2, View.EQUAL), (basis(0) * 2, 2, View.EQUAL),
                                      (np.ones(768), 2, View.EQUAL), (basis(0) * np.nan, 2, View.SHORT),
                                      (basis(0), True, View.SHORT), (basis(0), 513, View.SHORT),
                                      (basis(0), 1, View.SHORT), (basis(0), 2, 'unknown')):
            with self.assertRaises(ValueError): represent(current, basis(1), tokens, view)

    def test_support_exclusion_preserves_original_vectors_and_no_root_fallback(self):
        vectors = {control: {'root': basis(0), 'keep': basis(1)} for control in Control}
        result = derived_vectors(vectors, {'keep'}, {'keep': 10})
        self.assertEqual(set(result), {*Control, *View})
        for values in result.values():
            self.assertEqual(set(values), {'keep'})
        self.assertIs(result[Control.CURRENT]['keep'], vectors[Control.CURRENT]['keep'])

    def test_generic_summary_preserves_default_results_and_explicit_deltas(self):
        rows = [{'groups': ['a', 'b'], 'overlap': True, 'scores': {'current': 1., 'mean': 0., 'joined': 1., View.SHORT: 1.}},
                {'groups': ['a', 'c'], 'overlap': False, 'scores': {'current': 0., 'mean': 1., 'joined': 0., View.SHORT: 0.}}]
        draws = [{'a': 1, 'b': 1, 'c': 1}]
        original = summarize(rows, 'overlap', draws)
        expanded = summarize(rows, 'overlap', draws, (*Control, View.SHORT))
        for control in Control:
            self.assertEqual(original['metrics'][control], expanded['metrics'][control])
        for control in (Control.MEAN, Control.JOINED):
            self.assertEqual(original['paired_replicate_deltas'][0][control], expanded['paired_replicate_deltas'][0][control])
        self.assertEqual(expanded['paired_replicate_deltas'][0][View.SHORT], {'ap': 0., 'auc': 0.})
        for controls in ((), (Control.MEAN,), (Control.CURRENT, Control.CURRENT)):
            with self.assertRaises(ValueError): summarize(rows, 'overlap', draws, controls)
