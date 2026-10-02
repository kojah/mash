"""Synthetic metric/parser fixtures; never actual topic-quality annotations."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from unittest import TestCase
from topic_quality import parse,score,intervals


def fixture():
    threads=[]
    for i in range(20):
        tags=[{'id':str(j),'accurate':True,'specific':True,'source_supported':True,'evidence_source_ids':['a']} for j in range(3)]
        subjects=[{'id':str(j),'importance':'primary' if j<2 else 'secondary','uncertain':False,
            'evidence_source_ids':['a'],'matched_tag_id':str(j)} for j in range(3)]
        threads.append({'id':str(i),'provider':'codex' if i%2 else 'claude-code','source_ids':['a'],
            'emitted_tag_ids':['0','1','2'],'tags':tags,'subjects':subjects})
    return {'schema_version':1,'scope':'topic-quality-declared-counts-v1','packet_sha256':'0'*64,'candidate_sha256':'1'*64,
        'exposure':'declared-heldout-human','reviewer':{'identity':'synthetic-fixture-only','human_attestation':True,
            'prior_source_exposure':False,'outputs_seen_before_subjects':False},'threads':threads}


def encoded(value):return json.dumps(value).encode()


class TopicQualityTests(TestCase):
    def test_declared_counts_never_establish_gold_or_promotion(self):
        result=score(parse(encoded(fixture())))
        self.assertEqual(result['review_sha256'],hashlib.sha256(encoded(fixture())).hexdigest())
        self.assertTrue(result['declared_quality_gate_met'])
        for field in ['independence_verified','source_provenance_verified','semantic_quality_verified','production_selected']:
            self.assertFalse(result[field])
        self.assertEqual(result['metrics']['secondary_coverage']['denominator'],20)

    def test_exact_thresholds_and_abstentions_count_as_misses(self):
        value=fixture()
        for i in range(8):value['threads'][i]['subjects'][0]['matched_tag_id']=None
        for i in range(8):value['threads'][i]['subjects'][2]['matched_tag_id']=None
        result=score(parse(encoded(value)))
        self.assertTrue(result['metrics']['primary_coverage']['target_met'])
        self.assertTrue(result['metrics']['secondary_coverage']['target_met'])
        value['threads'][8]['subjects'][2]['matched_tag_id']=None
        self.assertFalse(score(parse(encoded(value)))['metrics']['secondary_coverage']['target_met'])
        value=fixture()
        for i in range(9):
            value['threads'][i]['tags'][0]['specific']=False
            value['threads'][i]['subjects'][0]['matched_tag_id']=None
        self.assertTrue(score(parse(encoded(value)))['metrics']['tag_precision']['target_met'])
        value['threads'][9]['tags'][0]['specific']=False;value['threads'][9]['subjects'][0]['matched_tag_id']=None
        self.assertFalse(score(parse(encoded(value)))['metrics']['tag_precision']['target_met'])

    def test_empty_ambiguity_exposure_and_padding_cannot_pass_scope(self):
        value=fixture();value['threads']=[];result=score(parse(encoded(value)))
        self.assertFalse(result['declared_quality_gate_met']);self.assertIsNone(result['metrics']['tag_precision']['value'])
        for field in ['human_attestation','prior_source_exposure','outputs_seen_before_subjects']:
            value=fixture();value['reviewer'][field]=field!='human_attestation'
            self.assertFalse(score(parse(encoded(value)))['declared_review_eligible'])
        value=fixture();value['exposure']='exposed-development'
        self.assertFalse(score(parse(encoded(value)))['declared_review_eligible'])
        value=fixture()
        for subject in value['threads'][0]['subjects']:subject.update(uncertain=True,matched_tag_id=None)
        result=score(parse(encoded(value)));self.assertEqual(result['evaluation_scope']['uncertain_subjects'],3)
        self.assertFalse(result['minimum_scope_met'])

    def test_scoped_known_complete_distinct_judgments_are_required(self):
        mutations=[lambda v:v['threads'][0]['tags'].pop(),
            lambda v:v['threads'][0]['tags'][0].update(accurate=1),
            lambda v:v['threads'][0]['tags'][0].update(evidence_source_ids=['foreign']),
            lambda v:v['threads'][0]['subjects'][0].update(evidence_source_ids=[]),
            lambda v:v['threads'][0]['subjects'][0].update(matched_tag_id='1'),
            lambda v:v['threads'][0]['subjects'][0].update(matched_tag_id='unknown'),
            lambda v:v['threads'][0]['tags'][0].update(source_supported=False),
            lambda v:v['threads'].append(copy.deepcopy(v['threads'][0])),
            lambda v:v['threads'][0].update(provider='unknown'),lambda v:v.update(packet_sha256='invalid')]
        for mutate in mutations:
            value=fixture();mutate(value)
            with self.assertRaises(ValueError):parse(encoded(value))
        with self.assertRaises(ValueError):parse(b'{"schema_version":1,"schema_version":1}')

    def test_bootstrap_is_deterministic_and_reports_undefined_draws(self):
        review=parse(encoded(fixture()))
        self.assertEqual(intervals(review.threads,100),intervals(review.threads,100))
        self.assertEqual(intervals((),100)[0]['valid_resamples'],0)
        self.assertEqual(intervals(review.threads,100)[0]['lower'],1.)

    def test_large_original_source_scope_is_supported_without_duplicate_ids(self):
        value=fixture();value['threads'][0]['source_ids']+=['source-'+str(i) for i in range(100)]
        self.assertEqual(len(parse(encoded(value)).threads),20)
        value['threads'][0]['source_ids'].append('a')
        with self.assertRaises(ValueError):parse(encoded(value))

    def test_cli_schema_errors_are_actionable_without_private_fragments(self):
        value=fixture();value['threads'][0]['tags'][0]['id']='private-marker'
        with TemporaryDirectory() as folder:
            path=Path(folder)/'review.json';path.write_bytes(encoded(value))
            result=subprocess.run([sys.executable,str(Path(__file__).with_name('topic_quality.py')),'--review',str(path)],capture_output=True,text=True)
        self.assertEqual(result.returncode,1)
        self.assertEqual(json.loads(result.stderr)['message'],'unique emitted-tag judgments required')
        self.assertNotIn('private-marker',result.stderr);self.assertEqual(result.stdout,'')
