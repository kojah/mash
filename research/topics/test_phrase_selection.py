from unittest import TestCase
from topic_phrases import PhraseSource
from phrase_selection import select_source_coverage,Selection
from topic_phrase_store import PhraseStore
from topic_phrase_audit import audit_snapshot
from copy import deepcopy
from phrase_review_evaluation import digest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch
import hashlib
from topic_background import EnglishBackground


class PhraseSelectionTests(TestCase):
    def test_phatic_candidates_abstain_but_brief_technical_inputs_survive(self):
        store=PhraseStore('fixture')
        store.upsert(PhraseSource('a','t','p','Yes, please!'))
        store.upsert(PhraseSource('b','t','p','Redis?'))
        store.upsert(PhraseSource('c','u','p','HNSW'))
        store.upsert(PhraseSource('d','v','p','Postgres schemas'))
        snapshot=store.snapshot(Selection.PHATIC_COVERAGE)
        self.assertEqual(snapshot['memberships']['a'],[])
        self.assertTrue(snapshot['memberships']['b']);self.assertTrue(snapshot['memberships']['c'])
        self.assertEqual(store.records['a'].text,'Yes, please!')
        phrases={topic['phrase'] for topic in snapshot['topics']}
        self.assertIn('redis',phrases);self.assertIn('hnsw',phrases)
        self.assertFalse(any(set(term.split())&{'yes','please'} for term in phrases))

    def test_phatic_atom_filter_preserves_technical_token_boundaries(self):
        store=PhraseStore('fixture')
        for identity,term in [('a','no_std'),('b','hello-world'),('c','yes_client')]:
            store.upsert(PhraseSource(identity,identity,'p',term))
        result=store.snapshot(Selection.PHATIC_COVERAGE)
        atoms={atom for topic in result['topics'] for atom in topic['phrase'].split()}
        self.assertTrue({'no_std','hello-world','yes_client'}<=atoms)

    def test_input_hashing_streams_large_files(self):
        with TemporaryDirectory() as directory:
            path=Path(directory)/'source.bin';contents=b'data'*100_000;path.write_bytes(contents)
            with patch.object(Path,'read_bytes',side_effect=AssertionError('whole-file allocation')):
                self.assertEqual(digest(path),hashlib.sha256(contents).hexdigest())

    def test_candidates_from_brief_unit_remain_eligible_and_duplicate_vote_is_single(self):
        rows=[PhraseSource('a','t','p','long'),PhraseSource('b','t','p','brief')]
        tokens={'a':{'alpha':1,'beta':1},'b':{'gamma':1}}
        selected=select_source_coverage([10.,9.,1.],['alpha','beta','gamma'],rows,tokens,2)
        self.assertEqual([row['term'] for row in selected],['alpha','gamma'])
        duplicate=PhraseSource('c','t','p','long');tokens['c']=tokens['a']
        self.assertEqual(selected,select_source_coverage([10.,9.,1.],['alpha','beta','gamma'],rows+[duplicate],tokens,2))

    def test_store_policy_is_explicit_and_preserves_complete_source_scope(self):
        store=PhraseStore('fixture');store.upsert(PhraseSource('a','t','p','redis and postgres'))
        result=store.snapshot(Selection.SOURCE_COVERAGE)
        self.assertEqual(set(result['memberships']),{'a'});self.assertEqual(result['parameters']['source_unit_candidates'],2)
        self.assertNotIn('selection',store.snapshot()['parameters'])
        with self.assertRaises(ValueError):store.snapshot('source_unit_candidate_coverage')

    def test_independent_audit_rejects_weight_and_evidence_corruption(self):
        rows=[PhraseSource('a','t','p','redis redis postgres'),PhraseSource('b','t','p','sqlite'),
              PhraseSource('c','u','p','redis vectors'),PhraseSource('d','v','p','postgres schemas')]
        store=PhraseStore('fixture')
        for row in rows:store.upsert(row)
        ledger=[dict(id=row.id,thread=row.thread,provider=row.provider,text=row.text) for row in rows]
        for mode in Selection:
            background=EnglishBackground({'redis':1e-6,'schemas':1e-4},'0'*64) if mode==Selection.BACKGROUND_COVERAGE else None
            snapshot=store.snapshot(mode,background)
            probabilities=background.frequencies if background else None
            self.assertEqual(audit_snapshot(ledger,snapshot,probabilities)['maximum_score_error'],0.)
            corrupt=deepcopy(snapshot);corrupt['threads'][0]['tags'][0]['score']+=.1
            with self.assertRaises(AssertionError):audit_snapshot(ledger,corrupt,probabilities)
            corrupt=deepcopy(snapshot);corrupt['memberships']['a']=[]
            with self.assertRaises(AssertionError):audit_snapshot(ledger,corrupt,probabilities)

    def test_background_information_is_bounded_and_phrase_mean_is_explicit(self):
        frame=EnglishBackground({'generic':.01,'technical':1e-7},'0'*64)
        weights=frame.factors(['generic','technical','generic technical','unknown'])
        self.assertAlmostEqual(weights['generic'],2/9);self.assertAlmostEqual(weights['technical'],7/9)
        self.assertAlmostEqual(weights['generic technical'],.5);self.assertEqual(weights['unknown'],1.)
        with self.assertRaises(ValueError):EnglishBackground({'bad':float('nan')},'0'*64).factors(['bad'])
        with self.assertRaises(ValueError):PhraseStore('fixture').snapshot(Selection.BACKGROUND_COVERAGE)
