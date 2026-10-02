from dataclasses import replace
import unittest

import numpy as np

from embedding_windows import Field, Window
from window_inference import inputs, pooling


def unit(ids):
    return Window('source', Field.BODY, 0, 0, 1, 0, 1, 'hash', tuple(ids), (0,) * len(ids), len(ids) - 2)


class WindowInferenceTests(unittest.TestCase):
    def test_padding_masks_and_source_token_identity(self):
        output = inputs([unit([101, 3, 102]), unit([101, 4, 5, 6, 102])])
        np.testing.assert_array_equal(output['input_ids'], [[101, 3, 102, 0, 0], [101, 4, 5, 6, 102]])
        np.testing.assert_array_equal(output['attention_mask'], [[1, 1, 1, 0, 0], [1, 1, 1, 1, 1]])
        self.assertTrue(all(value.dtype == np.int64 for value in output.values()))

    def test_pooling_ignores_padding_and_matches_singleton_mean(self):
        hidden = np.arange(2*5*384, dtype=np.float32).reshape(2, 5, 384)
        mask = np.array([[1, 1, 1, 0, 0], [1, 1, 1, 1, 1]])
        actual = pooling(hidden, mask)
        np.testing.assert_array_equal(actual[0], hidden[0, :3].mean(0, dtype=np.float32))
        np.testing.assert_array_equal(actual[1], hidden[1].mean(0, dtype=np.float32))
        hidden[0, 3:] = -1e10
        np.testing.assert_array_equal(pooling(hidden, mask), actual)

    def test_invalid_windows_and_model_outputs_rejected(self):
        for units in ([], [unit([101])], [unit([101, 102])] * 17,
                      [replace(unit([101, 102]), types=())], [unit(range(257))]):
            with self.assertRaises(ValueError): inputs(units)
        for hidden, mask in ((np.zeros((1,2,383)), np.ones((1,2))),
                             (np.full((1,2,384), np.nan), np.ones((1,2))),
                             (np.ones((1,2,384)), np.zeros((1,2))),
                             (np.ones((1,2,384)), np.full((1,2), .5))):
            with self.assertRaises(ValueError): pooling(hidden, mask)


if __name__ == '__main__':
    unittest.main()
