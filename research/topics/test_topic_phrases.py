import copy
import json
from unittest import TestCase
from topic_phrases import PhraseSource,Support
from topic_phrase_store import PhraseStore
from topic_phrase_view import page


class PhraseTopicTests(TestCase):
    def test_inspection_escapes_topic_and_source_evidence(self):
        source=PhraseSource('a','t','p','<img src=x onerror=alert(1)>')
        store=PhraseStore('fixture');store.upsert(source);snapshot=store.snapshot()
        if snapshot['topics']:snapshot['topics'][0]['phrase']='<script>alert(1)</script>'
        output=page({'forward':snapshot},{'baseline_checkpoint':{'records':[{'id':'a','text':source.text}]}})
        self.assertNotIn('<img',output);self.assertNotIn('<script>',output)
        self.assertIn('&lt;img',output);self.assertIn('default-src',output)

    def test_duplicate_inputs_do_not_create_independent_recurrence(self):
        store=PhraseStore('fixture')
        for identity,thread in [('a','one'),('b','two')]:store.upsert(PhraseSource(identity,thread,'p','redis, caching'))
        first=store.snapshot();self.assertTrue(first['topics'])
        self.assertTrue(all(topic['support']==Support.PROVISIONAL for topic in first['topics']))
        store.upsert(PhraseSource('c','one','p','redis, caching'))
        duplicate=store.snapshot()
        self.assertEqual(first['threads'][0]['tags'],duplicate['threads'][0]['tags'])
        self.assertEqual(first['memberships']['a'],duplicate['memberships']['a'])
        self.assertEqual([t['id'] for t in first['topics']],[t['id'] for t in duplicate['topics']])
        store.upsert(PhraseSource('d','two','p','redis, persistence'))
        self.assertTrue(any(topic['phrase']=='redis' and topic['support']==Support.RECURRING for topic in store.snapshot()['topics']))

    def test_stopword_boundaries_stable_namespace_and_multiple_tags(self):
        store=PhraseStore('fixture');store.upsert(PhraseSource('a','one','p','redis and postgres'))
        result=store.snapshot();self.assertEqual({topic['phrase'] for topic in result['topics']},{'redis','postgres'})
        self.assertEqual(len(result['memberships']['a']),2)
        other=PhraseStore('other');other.upsert(store.records['a'])
        self.assertFalse({topic['id'] for topic in result['topics']}&{topic['id'] for topic in other.snapshot()['topics']})

    def test_edit_delete_restart_and_invalid_replacement_are_atomic(self):
        store=PhraseStore('fixture');store.upsert(PhraseSource('a','t','p','redis sockets'))
        checkpoint=json.loads(json.dumps(store.checkpoint()));self.assertEqual(store.snapshot(),PhraseStore.restore(checkpoint).snapshot())
        store.upsert(PhraseSource('a','t','p','postgres pool'));self.assertNotEqual(checkpoint,store.checkpoint())
        current=store.checkpoint()
        for value in [PhraseSource('b','t','other','text'),PhraseSource('a','t','p','x'*262145)]:
            with self.assertRaises(ValueError):store.upsert(value)
            self.assertEqual(store.checkpoint(),current)
        store.delete('a');self.assertEqual(store.snapshot()['topics'],[])
        corrupt=copy.deepcopy(checkpoint);corrupt['records'][0]['version']=True
        with self.assertRaises(ValueError):PhraseStore.restore(corrupt)
