import copy
import unittest

from compare_field_labels import compare
from feedback_fields import Selection, parse
from field_labels import Score, label
from pair_review import Key


class FieldRepeatTests(unittest.TestCase):
    def report(self):
        key = Key('t', 0, 0)
        text = '## Page Feedback: /docs\n### 1. database\n**Feedback:** timeout\n'
        units, groups = {key: text}, [(key,)]
        coverage = [{'key': {'thread': 't', 'ordinal': 0, 'index': 0}}]
        return {'schema_version': 1, 'scope': 'development-fixed-membership-feedback-field-labels',
                'groups': 1, 'records': 1, 'elapsed_ms_after_imports': 1, 'peak_rss_mib': 1,
                'linux_vm_hwm_mib': 1, 'captures': [
                    {'selection': mode, 'coverage': coverage, 'label_ms': 1,
                     **label(units, groups, {key: parse(text)}, mode)} for mode in Selection]}

    def test_measurements_may_change_but_scores_and_sources_may_not(self):
        first = self.report()
        second = copy.deepcopy(first)
        second['elapsed_ms_after_imports'] = 9
        second['linux_vm_hwm_mib'] = 9
        second['captures'][0]['label_ms'] = 9
        self.assertTrue(compare(first, second)['identical_nonmeasurement_fields'])
        second['captures'][0]['clusters'][0]['labels'][Score.CTFIDF][0]['score'] += .01
        with self.assertRaisesRegex(ValueError, 'nonmeasurement'):
            compare(first, second)

    def test_reject_incomplete_grids_even_when_both_repeats_match(self):
        for mutation in ('selection', 'scorer', 'coverage', 'membership'):
            report = self.report()
            if mutation == 'selection':
                report['captures'].pop()
            elif mutation == 'scorer':
                report['captures'][0]['clusters'][0]['labels'].pop(Score.IDF)
            elif mutation == 'coverage':
                report['captures'][0]['coverage'] *= 2
            else:
                report['captures'][1]['clusters'][0]['members'][0]['ordinal'] = 1
            with self.assertRaises(ValueError, msg=mutation):
                compare(report, copy.deepcopy(report))
