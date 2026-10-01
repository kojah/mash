from copy import deepcopy
import unittest

from compare_payloads import compare
from unit_grouping import Method


class PayloadRepeatTests(unittest.TestCase):
    def fixture(self):
        return {'schema_version': 1, 'scope': 'development-explicit-feedback-payload-pilot',
                'elapsed_ms_after_imports': 1, 'cpu_ms': 2, 'peak_rss_mib': 3, 'matched_records': 1,
                'captures': [{'mode': mode, 'selection_ms': 2,
                              'coverage': [{'key': {'thread': 't', 'ordinal': 1, 'index': 0}, 'encoded': True}],
                              'reports': [{'method': m, 'reverse': reverse, 'metrics': {'AP': .5}}
                                          for m in Method for reverse in (False, True)]}
                             for mode in ('intact', 'explicit-feedback-payload')]}

    def test_repeat_requires_complete_grid_matching_support_and_unchanged_predictions(self):
        first = self.fixture()
        second = deepcopy(first)
        second['captures'][0]['selection_ms'] = 4
        self.assertEqual(compare(first, second)['controls'], 16)
        for mutation in ('prediction', 'grid', 'support'):
            changed = deepcopy(second)
            if mutation == 'prediction':
                changed['captures'][0]['reports'][0]['metrics']['AP'] = .6
            elif mutation == 'grid':
                changed['captures'][0]['reports'][0] = deepcopy(changed['captures'][0]['reports'][1])
            else:
                changed['captures'][0]['coverage'][0]['encoded'] = False
            with self.assertRaises(ValueError):
                compare(first, changed)
