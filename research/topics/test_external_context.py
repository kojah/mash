import copy
import unittest

import numpy as np

from external_context import Control, compare, inputs, metrics, pairs
from multiwoz import Speaker, Turn


class ExternalContextTests(unittest.TestCase):
    def test_original_text_only_and_root_abstention(self):
        current = Turn('d', 2, Speaker.USER, '  exact\nrequest ')
        preceding = Turn('d', 1, Speaker.SYSTEM, 'original reply')
        self.assertEqual(inputs(current, preceding, Control.CURRENT), (current.text,))
        self.assertEqual(inputs(current, preceding, Control.MEAN), (current.text, preceding.text))
        self.assertEqual(inputs(current, preceding, Control.JOINED), ('original reply\n\n  exact\nrequest ',))
        self.assertEqual(inputs(Turn('d', 0, Speaker.USER, 'root'), None, Control.JOINED), ())
        with self.assertRaises(ValueError): inputs(current, None, Control.MEAN)
        for bad in (Turn('other', 1, Speaker.SYSTEM, 'wrong'), Turn('d', 0, Speaker.SYSTEM, 'old'),
                    Turn('d', 1, Speaker.USER, 'wrong role')):
            with self.assertRaises(ValueError): inputs(current, bad, Control.MEAN)

    def test_pair_support_excludes_aliases_unknown_and_empty_proxy(self):
        anchors = {'a': {'group': 'one', 'active': ['hotel'], 'unknown': []},
                   'alias': {'group': 'one', 'active': ['hotel'], 'unknown': []},
                   'b': {'group': 'two', 'active': ['hotel', 'train'], 'unknown': []},
                   'c': {'group': 'three', 'active': ['restaurant'], 'unknown': []},
                   'empty': {'group': 'four', 'active': [], 'unknown': []},
                   'unknown': {'group': 'five', 'active': ['hotel'], 'unknown': [['taxi', 'find_taxi']]}}
        vectors = {key: np.array([1., 0.]) for key in anchors}
        rows = pairs(anchors, vectors, set(anchors))
        self.assertEqual(len(rows), 5)
        self.assertEqual(sum(r['overlap'] for r in rows), 2)
        self.assertFalse(any(r['left'] == 'a' and r['right'] == 'alias' for r in rows))
        self.assertEqual(len(pairs(anchors, vectors, {'a', 'b'})), 1)
        self.assertIsNone(metrics(pairs(anchors, vectors, {'a', 'b'}))['roc_auc'])

    def test_repeat_includes_complete_scores_and_support_not_timings(self):
        result = {'timings': {'total_ms': 1}, 'results': {'scores': [0.5]}, 'support': ['a']}
        repeat = copy.deepcopy(result)
        repeat['timings']['total_ms'] = 2
        compare(result, repeat)
        for key, changed in [('results', {'scores': [0.6]}), ('support', [])]:
            altered = copy.deepcopy(repeat)
            altered[key] = changed
            with self.assertRaises(ValueError): compare(result, altered)
