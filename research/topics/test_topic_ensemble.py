import copy
import json
from unittest import TestCase
import numpy as np
from topic_stream import Source
from topic_ensemble import Corroborator


class EnsembleTests(TestCase):
    def test_own_thread_is_excluded_and_rare_subjects_remain_provisional(self):
        model = Corroborator()
        model.upsert(Source('a', 'one', 'p', 'redis persistence', np.array([1., 0.])))
        self.assertEqual(model.judge('redis', 'one')['state'], 'provisional')
        model.upsert(Source('b', 'two', 'p', 'redis pricing', np.array([0., 1.])))
        self.assertFalse(model.judge('redis', 'one')['keep'])
        self.assertEqual(model.judge('redis', 'one')['cosine'], 0.)
        self.assertEqual(model.judge('persistence', 'one')['state'], 'provisional')

    def test_duplicate_edits_deletions_restart_and_order(self):
        model = Corroborator()
        rows = [Source('a', 'one', 'p', 'redis persistence', [1., 0.]),
                Source('b', 'two', 'p', 'redis recovery', [1., 0.])]
        for row in rows:
            model.upsert(row)
        before = model.judge('redis', 'one')
        self.assertTrue(before['keep'])
        self.assertFalse(model.upsert(rows[0]))
        model.upsert(Source('duplicate', 'one', 'p', rows[0].text, rows[0].vector))
        self.assertEqual(before, model.judge('redis', 'one'))
        model.delete('duplicate')
        model.upsert(Source('b', 'two', 'p', 'redis recovery', [0., 1.]))
        self.assertFalse(model.judge('redis', 'one')['keep'])
        restored = Corroborator.restore(json.loads(json.dumps(model.checkpoint())))
        reverse = Corroborator.restore(list(reversed(model.checkpoint())))
        self.assertEqual(model.judge('redis', 'one'), restored.judge('redis', 'one'))
        self.assertEqual(model.judge('redis', 'one'), reverse.judge('redis', 'one'))
        restored.delete('b')
        self.assertEqual(restored.judge('redis', 'one')['state'], 'provisional')

    def test_invalid_replacements_are_atomic_and_undefined_geometry_abstains(self):
        model = Corroborator()
        model.upsert(Source('a', 'one', 'p', 'redis persistence', [0., 0.]))
        model.upsert(Source('b', 'two', 'p', 'redis recovery', [1., 0.]))
        self.assertFalse(model.judge('redis', 'one')['keep'])
        before = copy.deepcopy(model.checkpoint())
        for bad in [Source('a', 'one', 'p', 'redis', [float('nan'), 0.]),
                    Source('a', 'one', 'p', 'redis', [1.e308, 0.]),
                    Source('c', 'one', 'p', 'redis persistence', [1., 0.]),
                    Source('a', 'two', 'other', 'redis', [1., 0.])]:
            with self.assertRaises(ValueError):
                model.upsert(bad)
            self.assertEqual(model.checkpoint(), before)

    def test_filter_preserves_evidence_ids_and_does_not_refill_or_mutate_input(self):
        model = Corroborator()
        model.upsert(Source('a', 'one', 'p', 'redis persistence', [1., 0.]))
        model.upsert(Source('b', 'two', 'p', 'redis pricing', [0., 1.]))
        snapshot = {'topics': [{'id': 'redis', 'phrase': 'redis',
            'support': 'cross_thread_and_distinct_input', 'sources': ['a', 'b'], 'threads': ['one', 'two']}],
            'threads': [{'thread': 'one', 'sources': ['a'], 'tags': [{'topic': 'redis', 'score': 1.}]},
                        {'thread': 'two', 'sources': ['b'], 'tags': [{'topic': 'redis', 'score': 1.}]}],
            'memberships': {'a': [{'topic': 'redis', 'score': 1.}], 'b': [{'topic': 'redis', 'score': 1.}]},
            'parameters': {}, 'diagnostics': {}}
        original = copy.deepcopy(snapshot)
        result, decisions = model.apply(snapshot)
        self.assertEqual(snapshot, original)
        self.assertEqual(result['topics'], [])
        self.assertEqual(result['memberships'], {'a': [], 'b': []})
        self.assertTrue(all(not row['keep'] for row in decisions))
        self.assertEqual(result['diagnostics']['untagged_threads'], 2)
