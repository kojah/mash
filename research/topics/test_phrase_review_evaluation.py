from unittest import TestCase
import hashlib
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from phrase_review_evaluation import support_metrics,evaluate,Subject,Importance,load


class PhraseReviewTests(TestCase):
    def test_redacted_sensitive_records_stay_opaque_and_content_redaction_rejects(self):
        text='redis';fingerprint=hashlib.sha256(text.encode()).hexdigest();opaque='0'*64
        packet={'schema_version':1,'cases':[{'thread':'t','provider':'p','source_sha256':'1'*64,'turns':[
            {'id':'a','ordinal':0,'text':text,'text_sha256':fingerprint,'display_redacted':False},
            {'id':'b','ordinal':1,'text':'[redacted]','text_sha256':opaque,'display_redacted':True}]}]}
        review={'schema_version':1,'scope':'fixture','annotator':'fixture','cases':[{'thread':'t','source_sha256':'1'*64,
            'admission_judgments':[{'message':'a','ordinal':0,'kind':'content','text_sha256':fingerprint},
                                  {'message':'b','ordinal':1,'kind':'sensitive','text_sha256':opaque}],
            'subjects':[{'importance':'primary','subject':'redis','support_ordinals':[0]}]}]}
        with TemporaryDirectory() as folder:
            pp=Path(folder)/'packet.json';rp=Path(folder)/'review.json';pp.write_text(json.dumps(packet))
            review['packet_sha256']=hashlib.sha256(pp.read_bytes()).hexdigest();rp.write_text(json.dumps(review))
            sources,_,policy=load(pp,rp);self.assertEqual(len(sources),1)
            self.assertEqual(policy['opaque_sensitive_exclusions_not_rehashed'],1)
            review['cases'][0]['admission_judgments'][1]['kind']='content';rp.write_text(json.dumps(review))
            with self.assertRaises(ValueError):load(pp,rp)

    def test_support_f1_does_not_confuse_other_topic_evidence_with_subject_coverage(self):
        snapshot={'topics':[{'id':'topic','phrase':'redis','sources':['a','other-thread']}],
            'threads':[{'thread':'t','sources':['a','b'],'tags':[{'topic':'topic'}]}],
            'memberships':{'a':[{'topic':'topic'}],'b':[]}}
        rows,summary=evaluate(snapshot,[Subject('t',0,Importance.PRIMARY,'redis',frozenset(['a','b'])),
                                        Subject('t',1,Importance.SECONDARY,'postgres',frozenset(['b']))])
        self.assertEqual(rows[0]['any_phrase_evidence_recall'],.5)
        self.assertAlmostEqual(rows[0]['best_single_phrase_support']['f1'],2/3)
        self.assertEqual(summary['secondary']['subjects_with_no_literal_evidence'],1)
        self.assertFalse(rows[1]['subject_name_atom_hit'])

    def test_empty_and_overlapping_support(self):
        self.assertEqual(support_metrics({'a'},()),{'precision':0.,'recall':0.,'f1':0.})
        self.assertEqual(support_metrics({'a','b'},{'a','c'})['f1'],.5)
