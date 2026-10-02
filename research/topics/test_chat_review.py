"""Synthetic schema fixtures only; these are not actual human annotations."""
import copy
from dataclasses import is_dataclass
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from chat_review import decode,packet,summary,validate
from chat_review_models import Importance,MetadataField,TaskKind


def encode(value): return json.dumps(value,ensure_ascii=False).encode()


def fixture():
    def turn(identity,text,ordinal=0,redacted=False):
        return {'id':identity,'ordinal':ordinal,'role':'user','text':text,'display_redacted':redacted,
                'original_text_sha256':hashlib.sha256(text.encode()).hexdigest()}
    raw = encode({'schema_version':1,'scope':'source-only-coding-chat-development-pilot-not-heldout',
        'documents':[{'id':'a','turns':[turn('a1','Alpha source context'),turn('a2','withheld',1,True)]},
                     {'id':'b','turns':[turn('b1','Beta source context')]}]})
    source = packet(raw)
    def judgment(identity,tid,word):
        return {'id':identity,'sufficient_context':True,'subjects':[{'name':word,'importance':'primary',
            'broader_subjects':['Fixture parent'],'evidence':[{'turn_id':tid,'quote':word}],'uncertainty':''}],
            'navigation_tasks':[{'request':'Find fixture '+word,'kind':'specific-thread-recovery',
                'evidence':[{'turn_id':tid,'quote':word}],'constraints':{'provider':None,'time_range':None,'node':None},'unavailable_metadata':[]}],
            'notes':''}
    answer = {'schema_version':1,'scope':'source-only-coding-chat-development-pilot-not-heldout','packet_sha256':source.sha256,
        'reviewer':{'identity':'synthetic-unit-fixture','human_attestation':True,'prior_source_exposure':False,'candidate_output_exposure':False},
        'documents':[judgment('a','a1','Alpha'),judgment('b','b1','Beta')]}
    return raw,source,answer


class ChatReviewTests(unittest.TestCase):
    def test_valid_reviews_return_typed_models_not_gold_claims(self):
        raw,source,a = fixture(); result = validate(source,encode(a)); self.assertTrue(is_dataclass(result))
        self.assertIs(result.documents[0].subjects[0].importance,Importance.PRIMARY)
        self.assertIs(result.documents[0].tasks[0].kind,TaskKind.THREAD)
        s = summary(result); self.assertEqual((s['documents'],s['subjects'],s['tasks']),(2,2,2))
        self.assertFalse(s['independence_verified']); self.assertFalse(s['semantic_quality_established']); self.assertFalse(s['agent_utility_established'])

    def test_null_blank_answers_and_missing_or_duplicate_documents_are_rejected(self):
        raw,source,a = fixture()
        for transform in [lambda a:a['documents'][0].update(subjects=None),lambda a:a['documents'].pop(),
                          lambda a:a['documents'].append(copy.deepcopy(a['documents'][0])),lambda a:a['documents'][0].update(notes=None)]:
            candidate = copy.deepcopy(a); transform(candidate)
            with self.assertRaises(ValueError): validate(source,encode(candidate))
        a['reviewer']['identity'] = None
        with self.assertRaises(ValueError): validate(source,encode(a))

    def test_literal_types_packet_pins_and_exposure_are_required(self):
        raw,source,a = fixture()
        for transform in [lambda a:a.update(schema_version=True),lambda a:a.update(packet_sha256='0'*64),
                          lambda a:a['reviewer'].update(human_attestation=False),lambda a:a['reviewer'].update(human_attestation=1),
                          lambda a:a['reviewer'].update(prior_source_exposure=None),lambda a:a['documents'][0].update(sufficient_context='true')]:
            candidate = copy.deepcopy(a); transform(candidate)
            with self.assertRaises(ValueError): validate(source,encode(candidate))
        a['reviewer']['candidate_output_exposure'] = True
        self.assertFalse(summary(validate(source,encode(a)))['declared_no_prior_exposure'])

    def test_cross_document_changed_empty_and_redacted_evidence_fail(self):
        raw,source,a = fixture()
        for evidence in [[],[{'turn_id':'b1','quote':'Beta'}],[{'turn_id':'a1','quote':'alpha'}],
                         [{'turn_id':'a2','quote':'withheld'}],[{'turn_id':'a1','quote':''}],
                         [{'turn_id':'a1','quote':'Alpha'},{'turn_id':'a1','quote':'Alpha'}]]:
            candidate = copy.deepcopy(a); candidate['documents'][0]['subjects'][0]['evidence'] = evidence
            with self.assertRaises(ValueError): validate(source,encode(candidate))

    def test_unknown_enums_schema_and_self_ancestor_are_rejected(self):
        raw,source,a = fixture()
        for transform in [lambda a:a['documents'][0]['subjects'][0].update(importance='main'),
                          lambda a:a['documents'][0]['subjects'][0].update(broader_subjects=['Alpha']),
                          lambda a:a['documents'][0]['navigation_tasks'][0].update(kind='negative-query'),
                          lambda a:a['documents'][0]['subjects'][0].update(extra=True)]:
            candidate = copy.deepcopy(a); transform(candidate)
            with self.assertRaises(ValueError): validate(source,encode(candidate))

    def test_empty_judgments_are_explicit_with_notes(self):
        raw,source,a = fixture(); a['documents'][0].update(subjects=[],navigation_tasks=[],notes='Explicitly no supported subjects in this toy fixture.')
        self.assertEqual(summary(validate(source,encode(a)))['subjects'],1)
        a['documents'][0]['notes'] = ''
        with self.assertRaises(ValueError): validate(source,encode(a))

    def test_insufficient_context_requires_uncertainty_and_no_answerable_tasks(self):
        raw,source,a = fixture(); d = a['documents'][0]; d.update(sufficient_context=False,notes='Context is unavailable.')
        with self.assertRaises(ValueError): validate(source,encode(a))
        d['navigation_tasks'] = []; d['subjects'][0]['uncertainty'] = 'Cannot establish the requested relationship.'
        self.assertEqual(summary(validate(source,encode(a)))['insufficient_context'],1)
        d['subjects'][0]['uncertainty'] = ''
        with self.assertRaises(ValueError): validate(source,encode(a))

    def test_scoped_requests_cannot_claim_available_source_metadata(self):
        raw,source,a = fixture(); task = a['documents'][0]['navigation_tasks'][0]; task['kind'] = 'scoped-lookup'
        with self.assertRaises(ValueError): validate(source,encode(a))
        task['constraints']['provider'] = 'codex'
        with self.assertRaises(ValueError): validate(source,encode(a))
        task['unavailable_metadata'] = ['provider']; result = validate(source,encode(a))
        self.assertEqual(result.documents[0].tasks[0].constraints.unavailable,frozenset([MetadataField.PROVIDER]))
        self.assertEqual(summary(result)['tasks_with_unavailable_metadata'],1)
        task['unavailable_metadata'] = ['provider','provider']
        with self.assertRaises(ValueError): validate(source,encode(a))

    def test_time_ranges_have_timezone_and_order(self):
        raw,source,a = fixture(); task = a['documents'][0]['navigation_tasks'][0]
        task.update(kind='scoped-lookup',unavailable_metadata=['time_range'])
        task['constraints']['time_range'] = {'start':'2026-10-02T09:00:00+02:00','end':'2026-10-02T08:00:00Z'}
        self.assertIsNotNone(validate(source,encode(a)).documents[0].tasks[0].constraints.time_range)
        for period in [{'start':None,'end':None},{'start':'2026-10-02','end':None},
                       {'start':'2026-10-02T10:00:00Z','end':'2026-10-02T08:00:00Z'},
                       {'start':'2026-02-30T00:00:00Z','end':None}]:
            task['constraints']['time_range'] = period
            with self.assertRaises(ValueError): validate(source,encode(a))

    def test_packet_duplicate_identity_hash_and_order_drift_fail(self):
        raw,source,a = fixture(); original = decode(raw)
        for transform in [lambda p:p['documents'][1].update(id='a'),lambda p:p['documents'][1]['turns'][0].update(id='a1'),
                          lambda p:p['documents'][0]['turns'][0].update(text='Changed context'),
                          lambda p:p['documents'][0]['turns'][0].update(ordinal=True),
                          lambda p:p['documents'][0]['turns'].reverse()]:
            candidate = copy.deepcopy(original); transform(candidate)
            with self.assertRaises(ValueError): packet(encode(candidate))

    def test_duplicate_json_fields_nonfinite_and_invalid_utf8_are_rejected(self):
        for raw in [b'{"a":1,"a":2}',b'{"a":NaN}',b'{"a":Infinity}',b'\xff',b'['*2000+b']'*2000]:
            with self.assertRaises(ValueError): decode(raw)

    def test_review_row_order_does_not_change_typed_source_alignment(self):
        raw,source,a = fixture(); first = validate(source,encode(a)); a['documents'].reverse()
        self.assertEqual(first,validate(source,encode(a)))

    def test_cli_outputs_only_aggregate_counts_and_safe_errors(self):
        raw,source,a = fixture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); p = root/'packet.json'; r = root/'answers.json'; p.write_bytes(raw); r.write_bytes(encode(a))
            command = [sys.executable,str(Path(__file__).with_name('chat_review.py')),'--packet',str(p),'--answers',str(r)]
            success = subprocess.run(command,text=True,capture_output=True,check=True)
            self.assertNotIn('Alpha',success.stdout); self.assertNotIn('synthetic-unit-fixture',success.stdout)
            a['documents'][0]['subjects'][0]['evidence'][0]['quote'] = 'PRIVATE_INVALID_QUOTE'; r.write_bytes(encode(a))
            failure = subprocess.run(command,text=True,capture_output=True)
            self.assertEqual(failure.returncode,1); self.assertEqual(json.loads(failure.stderr)['error'],'invalid_review')
            self.assertNotIn('PRIVATE_INVALID_QUOTE',failure.stderr); self.assertNotIn('Traceback',failure.stderr)


if __name__ == '__main__': unittest.main()
