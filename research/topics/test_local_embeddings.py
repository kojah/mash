import unittest

import numpy as np

from local_embeddings import quantized_direction


class QuantizationTests(unittest.TestCase):
    def test_rust_tie_rounding_preserves_small_signed_components(self):
        vector = np.zeros(384, dtype=np.float32)
        vector[:3] = (127, 0.5, -0.5)
        direction = quantized_direction(vector)
        self.assertGreater(direction[1], 0)
        self.assertLess(direction[2], 0)
        self.assertAlmostEqual(direction[1] / direction[0], 1 / 127)

    def test_invalid_outputs_are_rejected(self):
        for vector in (np.zeros(384, dtype=np.float32), np.full(384, np.nan, dtype=np.float32), np.ones(383, dtype=np.float32)):
            with self.assertRaises(ValueError):
                quantized_direction(vector)
