import copy
import unittest

from dialogue_budget import Control, compare_case
from packet_contexts import capture_case
import test_packet_contexts


class DialogueBudgetTests(unittest.TestCase):
    def test_raw_control_exact_and_changed_control_explicit(self):
        fixture = test_packet_contexts.PacketContextTests()
        fixture.setUp()
        baseline = capture_case(fixture.case, fixture.messages, [3])
        result = compare_case(fixture.case, fixture.messages, [3], baseline)
        self.assertEqual(result[Control.RAW]['contexts'], baseline)
        self.assertEqual(result[Control.VISIBLE]['contexts'][0]['maximum_visible'], 16)
        self.assertEqual(result[Control.VISIBLE]['contexts'][0]['bounds'][0], 4096)
        self.assertNotIn('sibling', {m['id'] for m in result[Control.VISIBLE]['contexts'][0]['display']})
        changed = copy.deepcopy(baseline)
        changed[0]['display'][0]['text'] = 'edited task'
        with self.assertRaisesRegex(ValueError, 'frozen context'):
            compare_case(fixture.case, fixture.messages, [3], changed)
