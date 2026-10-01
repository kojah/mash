import copy
import json
import unittest

from dialogue_budget import Control
from packet_reference_review import availability, validate
from preceding_context import Message, Role, preceding
from reference_review import digest


class PacketReferenceTests(unittest.TestCase):
    def test_availability_is_not_automatic_resolution_and_requires_intact_text(self):
        packet, review, report, annotation = self.fixture()
        validate(json.dumps(report).encode(), packet, review, annotation)
        scores = availability(json.dumps(report).encode(), annotation)
        self.assertEqual(scores[Control.RAW]['all_cited_evidence_available'], 0)
        self.assertEqual(scores[Control.VISIBLE]['all_cited_evidence_available'], 1)
        report['cases'][0]['controls'][Control.VISIBLE]['contexts'][0]['display'][-1]['text'] = 'edited'
        self.assertEqual(availability(json.dumps(report).encode(), annotation)[Control.VISIBLE]['all_cited_evidence_available'], 0)

    def fixture(self):
        turn = {'id': 'yes', 'ordinal': 3, 'text_sha256': digest(b'yes'), 'text': 'yes', 'display_redacted': False}
        case = {'thread': 't', 'source_group': 'g', 'source_sha256': 'h', 'turns': [turn]}
        packet = json.dumps({'cases': [case]}).encode()
        row = {k: turn[k] for k in ('id', 'ordinal', 'text_sha256')}
        row.update(kind='referential', rationale='source review', facets=[])
        review = json.dumps({'schema_version': 1, 'scope': 'development-packet-origin-review',
                             'packets_sha256': digest(packet), 'annotator': 'exposed', 'facet_granularity': 'task',
                             'cases': [{**case, 'turns': [row]}]}).encode()
        context = preceding((Message('u', 0, Role.USER, -1, 'task', 'task'),
                             Message('a', 1, Role.ASSISTANT, 0, 'proposal', 'proposal'),
                             Message('yes', 3, Role.USER, 1, 'yes', 'yes')), 3,
                            maximum_records=4096, maximum_users=3, maximum_visible=16)
        report = {'schema_version': 1, 'scope': 'development-visible-dialogue-budget-control',
                  'input_sha256': {'packets': digest(packet), 'review': digest(review)},
                  'cases': [{'thread': 't', 'source_sha256': 'h', 'controls': {
                      Control.RAW: {'contexts': []}, Control.VISIBLE: {'contexts': [context]}}}]}
        raw = json.dumps(report).encode()
        annotation = {'schema_version': 1, 'scope': 'bounded-preceding-reference-review',
                      'annotator': 'exposed assistant', 'context_sha256': digest(raw),
                      'source_review_sha256': digest(review), 'judgments': [{
                          'thread': 't', 'ordinal': 3, 'anchor': context['anchor'],
                          'context_sha256': context['context_sha256'], 'resolution': 'resolved-task',
                          'evidence': [context['traversed'][0]], 'rationale': 'source proposal'}]}
        return packet, review, report, annotation

    def test_source_backed_evidence_and_complete_review(self):
        packet, review, report, annotation = self.fixture()
        self.assertEqual(validate(json.dumps(report).encode(), packet, review, annotation)[0].evidence_ordinals, (1,))
        for mutation in ('bounds', 'display', 'missing-review', 'grid'):
            changed, judged = copy.deepcopy(report), copy.deepcopy(annotation)
            context = changed['cases'][0]['controls'][Control.VISIBLE]['contexts'][0]
            if mutation == 'bounds':
                context['maximum_visible'] = 20
            elif mutation == 'display':
                context['display'][-1]['text'] = 'unrelated'
            elif mutation == 'grid':
                changed['cases'][0]['controls'].pop(Control.RAW)
            else:
                judged['judgments'] = []
            judged['context_sha256'] = digest(json.dumps(changed).encode())
            with self.assertRaises(ValueError, msg=mutation):
                validate(json.dumps(changed).encode(), packet, review, judged)
