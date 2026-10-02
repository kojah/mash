import copy
import json
from pathlib import Path
import tempfile
import unittest

from qmsum import Transition, load, membership, parse, transition


def row():
    return {'meeting_transcripts': [{'speaker': 'One', 'content': 'é🙂\r\n'}, {'speaker': '', 'content': ''},
                                    {'speaker': 'Two', 'content': 'later'}, {'speaker': 'One', 'content': 'last'}],
            'topic_list': [{'topic': 'Alpha', 'relevant_text_span': [['0', '1'], ['1', '2']]},
                           {'topic': 'Beta', 'relevant_text_span': [['2', '2']]}],
            'general_query_list': [{'query': 'whole', 'answer': 'annotated'}],
            'specific_query_list': [{'query': 'part', 'answer': 'answer', 'relevant_text_span': [['2', '3']]}]}


def encoded(value):return (json.dumps(value, ensure_ascii=False)+'\n').encode()


class QmsumTests(unittest.TestCase):
    def test_exact_original_ordinals_text_and_inclusive_overlaps(self):
        m = parse(encoded(row()))[0];members, duplicated = membership(m)
        self.assertEqual(m.turns[0].text, 'é🙂\r\n');self.assertEqual(m.turns[1].text, '')
        self.assertEqual(m.turns[1].speaker, '');self.assertEqual(members, [(0,), (0,), (0, 1), ()])
        self.assertEqual(duplicated, 1);self.assertEqual(m.topics[0].spans[0].end, 1)
        self.assertEqual(m.specific_queries[0].spans[0].end, 3)

    def test_annotation_payload_cannot_affect_membership(self):
        a = row();b = copy.deepcopy(a);b['topic_list'][0]['topic'] = 'different'
        b['specific_query_list'][0]['query'] = 'different';b['general_query_list'][0]['answer'] = 'different'
        self.assertEqual(membership(parse(encoded(a))[0]), membership(parse(encoded(b))[0]))

    def test_duplicate_source_meetings_and_duplicate_spans_are_preserved(self):
        data = row();data['topic_list'][0]['relevant_text_span'].append(['0', '1'])
        meetings = parse(encoded(data)+encoded(data))
        self.assertEqual([m.ordinal for m in meetings], [0, 1])
        self.assertEqual(meetings[0].raw_line_sha256, meetings[1].raw_line_sha256)
        self.assertEqual(len(meetings[0].topics[0].spans), 3)
        self.assertEqual(membership(meetings[0])[1], 2)

    def test_transition_gaps_and_overlap_are_distinct_from_singleton_switch(self):
        self.assertEqual(transition((), ()), Transition.UNKNOWN)
        self.assertEqual(transition((), (0,)), Transition.GAP)
        self.assertEqual(transition((0,), ()), Transition.GAP)
        self.assertEqual(transition((0, 1), (0, 1)), Transition.STABLE)
        self.assertEqual(transition((0,), (1,)), Transition.SINGLETON_SWITCH)
        self.assertEqual(transition((0,), (0, 1)), Transition.OVERLAP_CHANGE)

    def test_invalid_original_spans_fail_without_repair(self):
        for value in [[['3', '4']], [['2', '1']], [[0, '1']], [['-1', '1']], [['0', '1', '2']], []]:
            data = row();data['topic_list'][0]['relevant_text_span'] = value
            with self.assertRaises(ValueError):parse(encoded(data))

    def test_duplicate_keys_empty_lines_nonfinite_unknown_fields_rejected(self):
        for raw in [b'{}\n\n', b'{"topic_list":[],"topic_list":[]}\n', b'NaN\n']:
            with self.assertRaises(ValueError):parse(raw)
        data = row();data['native_id'] = 'invented'
        with self.assertRaises(ValueError):parse(encoded(data))

    def test_capture_header_rejects_held_out_scope_before_source_loading(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory);(root/'capture.json').write_text('{}')
            with self.assertRaises(ValueError):load(root)


if __name__ == '__main__':unittest.main()
