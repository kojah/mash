import unittest

import numpy as np
from scipy.sparse import csr_matrix

from question_label_balance import Scope, balanced_counts
from question_topic_labels import Features


class BalanceTests(unittest.TestCase):
    def fixture(self):
        return Features(['title', 'body'], csr_matrix([[2, 0], [0, 100], [0, 50]]),
                        {'title/q': 0, 'body/q/a': 1, 'body/q/b': 2}, {}, {}, {}, {}, np.empty((0, 2)))

    def test_long_body_and_repeated_title_do_not_change_equal_source_mass(self):
        feature = self.fixture()
        value = balanced_counts([['title/q', 'body/q/a', 'body/q/b']], feature, Scope.BALANCED)
        np.testing.assert_array_equal(value, [[.5, .5]])
        np.testing.assert_array_equal(balanced_counts([['title/q', 'body/q/a']], feature, Scope.BALANCED), value)

    def test_title_only_does_not_silently_include_body_or_change_scope(self):
        value = balanced_counts([['title/q', 'body/q/a']], self.fixture(), Scope.TITLE)
        np.testing.assert_array_equal(value, [[1., 0.]])
        with self.assertRaises(ValueError):
            balanced_counts([['title/q']], self.fixture(), 'unknown')

    def test_empty_title_is_explicit_not_a_title_only_body_fallback(self):
        feature = self.fixture()
        feature.matrix = csr_matrix([[0, 0], [0, 100], [0, 50]])
        np.testing.assert_array_equal(balanced_counts([['title/q', 'body/q/a']], feature, Scope.TITLE), [[0., 0.]])
        np.testing.assert_array_equal(balanced_counts([['title/q', 'body/q/a']], feature, Scope.BALANCED), [[0., 1.]])


if __name__ == '__main__':
    unittest.main()
