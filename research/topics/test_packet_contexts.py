import copy
import unittest

from episodes import Turn, source_hash
from packet_contexts import capture_case
from preceding_context import Message, Role
from reference_review import digest


class PacketContextTests(unittest.TestCase):
    def setUp(self):
        self.messages = (Message('u', 0, Role.USER, -1, 'task', 'task'),
                         Message('a', 1, Role.ASSISTANT, 0, 'proposal', 'proposal'),
                         Message('sibling', 2, Role.ASSISTANT, 0, 'unrelated', 'unrelated'),
                         Message('yes', 3, Role.USER, 1, 'yes', 'yes'))
        self.case = {'source_sha256': source_hash(tuple(Turn(m.id, m.ordinal, m.searchable, None)
                                                      for m in self.messages if m.role == Role.USER)),
                     'turns': [{'id': m.id, 'ordinal': m.ordinal, 'text_sha256': digest(m.searchable.encode())}
                               for m in self.messages if m.role == Role.USER]}

    def test_exact_source_scope_and_causal_branch(self):
        context, = capture_case(self.case, self.messages, [3])
        self.assertEqual([s['id'] for s in context['display']], ['u', 'a'])
        self.assertFalse(context['native_log_verified'])
        self.assertEqual(capture_case(self.case, self.messages, []), [])

    def test_changed_identity_missing_user_and_duplicate_anchor_rejected(self):
        changed = copy.deepcopy(self.case)
        changed['turns'][0]['id'] = 'other'
        for case, messages, anchors in ((changed, self.messages, [3]), (self.case, self.messages[1:], [3]),
                                        (self.case, self.messages, [3, 3]), (self.case, self.messages, [1])):
            with self.assertRaises(ValueError):
                capture_case(case, messages, anchors)
