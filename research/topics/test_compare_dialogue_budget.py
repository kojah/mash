import copy
import unittest

from compare_dialogue_budget import compare
from dialogue_budget import compare_case, Control
import test_packet_contexts
from packet_contexts import capture_case


class DialogueRepeatTests(unittest.TestCase):
    def test_only_timing_may_change_and_incomplete_equal_reports_fail(self):
        fixture = test_packet_contexts.PacketContextTests()
        fixture.setUp()
        controls = compare_case(fixture.case, fixture.messages, [3], capture_case(fixture.case, fixture.messages, [3]))
        report = {'schema_version': 1, 'scope': 'development-visible-dialogue-budget-control',
                  'cases': [{'thread': 't', 'controls': controls}]}
        second = copy.deepcopy(report)
        second['cases'][0]['controls'][Control.VISIBLE]['capture_ms'] += 3
        self.assertTrue(compare(report, second)['identical_nonmeasurement_fields'])
        for mutation in ('grid', 'anchor', 'duplicate', 'text'):
            broken = copy.deepcopy(report)
            if mutation == 'grid':
                broken['cases'][0]['controls'].pop(Control.RAW)
            elif mutation == 'duplicate':
                broken['cases'] *= 2
            elif mutation == 'anchor':
                broken['cases'][0]['controls'][Control.VISIBLE]['contexts'][0]['anchor']['id'] = 'changed'
            else:
                broken['cases'][0]['controls'][Control.VISIBLE]['contexts'][0]['display'][0]['text'] = 'changed'
                with self.assertRaises(ValueError):
                    compare(report, broken)
                continue
            with self.assertRaises(ValueError):
                compare(broken, copy.deepcopy(broken))
