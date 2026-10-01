import copy
import unittest

from compare_record_selection import METHODS, compare


class RecordRepeatTests(unittest.TestCase):
    def test_complete_grid_and_scores_survive_measurement_filter(self):
        score = {'status': 'ranked', 'ranking': [{'id': 'a', 'cosine': .5}],
                 'cited_record_hits': {'hit_at_1': True, 'hit_at_3': True}}
        first = {'schema_version': 1, 'scope': 'development-stored-reference-record-selection',
                 'inventory': [{'thread': 't', 'identity': {'id': 'u', 'role': 'user'}},
                               {'thread': 't', 'identity': {'id': 'a', 'role': 'assistant'}}],
                 'reports': [{'thread': 't', 'anchor': {'id': 'u'}, 'selection_ms': 1,
                              'methods': {m: copy.deepcopy(score) for m in METHODS}}],
                 'preflight_ms': 1, 'ranking_ms_with_validation': 1}
        second = copy.deepcopy(first)
        second['reports'][0]['selection_ms'] = 9
        self.assertTrue(compare(first, second)['identical_nonmeasurement_fields'])
        second['reports'][0]['methods']['recency-all']['ranking'][0]['cosine'] = .6
        with self.assertRaisesRegex(ValueError, 'nonmeasurement'):
            compare(first, second)
        for mutation in ('method', 'source', 'duplicate'):
            changed = copy.deepcopy(first)
            if mutation == 'method':
                changed['reports'][0]['methods'].pop('recency-all')
            elif mutation == 'source':
                changed['inventory'][1]['identity']['role'] = 'user'
            else:
                changed['reports'] *= 2
            with self.assertRaises(ValueError):
                compare(changed, copy.deepcopy(changed))
