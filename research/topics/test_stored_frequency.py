import unittest

from stored_frequency import frequency_weights


class FrequencyTests(unittest.TestCase):
    def test_frequency_counts_threads_and_keeps_common_source_inputs(self):
        weights, frequency = frequency_weights({'a': ['common', 'common', 'rare'], 'b': ['common']})
        self.assertEqual(frequency, {'common': 2, 'rare': 1})
        self.assertEqual(weights['rare'], 1.)
        self.assertGreater(weights['common'], 0.)
        self.assertLess(weights['common'], 1.)

    def test_source_order_does_not_change_frequency(self):
        self.assertEqual(frequency_weights({'a': ['x', 'y'], 'b': ['y', 'z']}),
                         frequency_weights({'b': ['z', 'y'], 'a': ['y', 'x']}))
        with self.assertRaises(ValueError):
            frequency_weights({'a': ['x']})


if __name__ == '__main__':
    unittest.main()
