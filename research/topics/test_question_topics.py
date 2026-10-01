import unittest

import numpy as np
from scipy.sparse import csr_matrix

from question_topic_labels import Features, topic_labels
from question_topics import naming_metrics, parent_memberships
from reference_review import digest


class QuestionTopicTests(unittest.TestCase):
    def fixture(self):
        return Features(['alpha', 'beta', 'title'], csr_matrix([[0, 0, 1], [1, 0, 0], [0, 1, 0]]),
                        {'t': 0, 'a': 1, 'b': 2}, {'t': 'title', 'a': 'alpha', 'b': 'beta'},
                        {}, {'q': ('t', 'a', 'b')}, {}, np.array([[1, 0, 0], [1, 1, 0]]))

    def test_units_allow_multiple_direct_topics_and_complete_partition_is_required(self):
        parents = {'a': 'q', 'b': 'q', 'c': 'r'}
        self.assertEqual(parent_memberships([['a', 'c'], ['b']], parents), {'q': [0, 1], 'r': [0]})
        for groups in [[['a'], ['b']], [['a', 'b'], ['a', 'c']]]:
            with self.assertRaises(ValueError):
                parent_memberships(groups, parents)

    def test_repeated_model_title_is_counted_once_per_parent_topic(self):
        f = self.fixture()
        result = topic_labels([['x', 'y']], {'x': ('t', 'a'), 'y': ('t', 'b')}, {'q': [0]}, f)
        self.assertEqual(result['clusters'][0]['sources'], ['a', 'b', 't'])
        self.assertEqual(result['counts_sha256'], digest(np.array([[1, 1, 1]], dtype='<i8').tobytes()))

    def test_reference_tags_do_not_change_labels_and_availability_is_source_only(self):
        f = self.fixture()
        result = topic_labels([['x']], {'x': ('t', 'a')}, {'q': [0]}, f)
        first = naming_metrics(['q'], {'q': ['alpha', 'outside']}, {'q': [0]}, result, f)
        second = naming_metrics(['q'], {'q': ['beta', 'outside']}, {'q': [0]}, result, f)
        self.assertEqual(first['ctfidf']['rows'][0]['available_tags'], ['alpha'])
        self.assertEqual(second['ctfidf']['rows'][0]['available_tags'], ['beta'])
        self.assertEqual(first['ctfidf']['totals']['union_hits'], 1)
        self.assertEqual(second['ctfidf']['totals']['union_hits'], 0)

    def test_unsupported_source_label_is_rejected(self):
        f = self.fixture()
        f.texts['a'] = 'unrelated'
        with self.assertRaisesRegex(ValueError, 'supported'):
            topic_labels([['x']], {'x': ('a',)}, {'q': [0]}, f)

    def test_parent_budget_uses_maximum_topic_term_score_without_duplicate_words(self):
        f = self.fixture()
        result = topic_labels([['x'], ['y']], {'x': ('t', 'a'), 'y': ('t', 'b')}, {'q': [0, 1]}, f)
        labels = result['parent_top8']['q']['ctfidf']
        self.assertEqual(len({r['term'] for r in labels}), len(labels))
        self.assertEqual({r['term'] for r in labels}, {'alpha', 'beta', 'title'})


if __name__ == '__main__':
    unittest.main()
