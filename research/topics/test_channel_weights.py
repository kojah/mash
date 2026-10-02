import unittest

import numpy as np

from channel_weights import View, matrix, weighted


class ChannelWeightTests(unittest.TestCase):
    def test_code_volume_does_not_dominate_equal_channel_view(self):
        title, prose, code = [[[1, 0, 0]], [[0, 1, 0]], [[0, 0, 1000]]]
        np.testing.assert_allclose(weighted(title, prose, code, View.BALANCED), [[1/3, 1/3, 1/3]])
        np.testing.assert_allclose(weighted(title, prose, code, View.FULL), [[.5, .5/1001, 500/1001]])
        np.testing.assert_allclose(weighted(title, prose, code, View.PROSE), [[.5, .5, 0]])

    def test_empty_channels_and_zero_evidence_have_explicit_fallback(self):
        title, prose, code = np.array([[1, 0], [0, 0], [0, 0]]), np.zeros((3, 2)), np.array([[0, 0], [0, 10], [0, 0]])
        np.testing.assert_allclose(weighted(title, prose, code, View.FULL), [[1, 0], [0, 1], [0, 0]])
        np.testing.assert_allclose(weighted(title, prose, code, View.PROSE), [[1, 0], [0, 0], [0, 0]])

    def test_input_counts_are_unchanged_and_invalid_matrices_rejected(self):
        values = np.array([[1., 3.]])
        before = values.copy()
        for view in View:
            weighted(values, values, values, view)
        np.testing.assert_array_equal(values, before)
        for invalid in ([[float('nan'), 1]], [[-1, 1]], [[1]], []):
            with self.assertRaises(ValueError):
                weighted(values, invalid, values, View.FULL)

    def test_matrix_uses_source_counts_only_and_rejects_unsupported_sources(self):
        records = {'a': {'status': 'supported', 'title_counts': {'topic': 2, 'other': 100},
                         'counts': {'prose': {'topic': 3}, 'block-code': {'topic': 4}}}}
        np.testing.assert_array_equal(matrix(records, ['a'], ['topic', 'absent'], 'title'), [[2, 0]])
        np.testing.assert_array_equal(matrix(records, ['a'], ['topic'], 'block-code'), [[4]])
        records['a']['status'] = 'unsupported'
        with self.assertRaises(ValueError):
            matrix(records, ['a'], ['topic'], 'title')


if __name__ == '__main__':
    unittest.main()
