"""Actual interpreter/disk restart checks of the unchanged research reference."""
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from unittest import TestCase

from phrase_selection import Selection
from topic_phrase_store import PhraseStore
from topic_phrases import PhraseSource


# Run the real reference in another interpreter, with no producer implementation
# copied into this test. The checkpoint is a research JSON file, not a production
# database or an atomic publication protocol.
WORKER = '''
import json
from pathlib import Path
import sys
from phrase_selection import Selection
from topic_phrase_store import PhraseStore
from topic_phrases import PhraseSource
checkpoint, operations, output = map(Path, sys.argv[1:])
model = PhraseStore.restore(json.loads(checkpoint.read_text()))
snapshots = [model.snapshot(selection=Selection.SOURCE_COVERAGE)]
changes = []
for operation in json.loads(operations.read_text()):
    if operation['kind'] == 'upsert':
        changes.append(model.upsert(PhraseSource(**operation['source'])))
    else:
        changes.append(model.delete(operation['id']))
    snapshots.append(model.snapshot(selection=Selection.SOURCE_COVERAGE))
output.write_text(json.dumps({'snapshots': snapshots, 'changes': changes,
                             'checkpoint': model.checkpoint()}, sort_keys=True))
'''


class PhraseProcessRestartTests(TestCase):
    def run_worker(self, directory, checkpoint, operations):
        checkpoint_path = directory / 'checkpoint.json'
        operations_path = directory / 'operations.json'
        output_path = directory / 'output.json'
        checkpoint_path.write_text(json.dumps(checkpoint))
        operations_path.write_text(json.dumps(operations))
        result = subprocess.run(
            [sys.executable, '-B', '-c', WORKER, str(checkpoint_path),
             str(operations_path), str(output_path)],
            cwd=Path(__file__).resolve().parent, capture_output=True,
            text=True, timeout=30, check=False)
        return result, output_path

    def test_restart_matches_uninterrupted_edits_deletes_and_new_subjects(self):
        model = PhraseStore('node-group/reference-v1')
        redis = PhraseSource('redis-original', 'claude-thread', 'claude',
                             'redis persistence recovery snapshots')
        sqlite = PhraseSource('sqlite-original', 'codex-thread', 'codex',
                              'sqlite query planner indexes')
        for source in [redis, sqlite]:
            model.upsert(source)
        checkpoint = model.checkpoint()
        before = model.snapshot(selection=Selection.SOURCE_COVERAGE)
        redis_ids = {topic['phrase']: topic['id'] for topic in before['topics']}
        operations = [
            {'kind': 'upsert', 'source': vars(redis)},
            {'kind': 'upsert', 'source': {**vars(redis), 'id': 'redis-copy'}},
            {'kind': 'upsert', 'source': {**vars(sqlite),
                'text': 'postgres transaction isolation deadlocks'}},
            {'kind': 'delete', 'id': 'redis-original'},
            {'kind': 'upsert', 'source': vars(PhraseSource(
                'qdrant-new', 'new-thread', 'codex',
                'qdrant vector collection payload filtering'))},
            {'kind': 'delete', 'id': 'missing'},
            {'kind': 'delete', 'id': 'redis-copy'},
        ]
        expected = [before]
        changes = []
        for operation in operations:
            changes.append(model.upsert(PhraseSource(**operation['source']))
                           if operation['kind'] == 'upsert'
                           else model.delete(operation['id']))
            expected.append(model.snapshot(selection=Selection.SOURCE_COVERAGE))
        with TemporaryDirectory() as temporary:
            directory = Path(temporary)
            result, output_path = self.run_worker(directory, checkpoint, operations)
            self.assertEqual(result.returncode, 0, result.stderr)
            actual = json.loads(output_path.read_text())
            self.assertEqual(actual['snapshots'], expected)
            self.assertEqual(actual['changes'], changes)
            self.assertEqual(actual['checkpoint'], model.checkpoint())
            # A second fresh process restores the post-edit persisted state.
            result, output_path = self.run_worker(directory, actual['checkpoint'], [])
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(output_path.read_text())['snapshots'], [expected[-1]])
        self.assertFalse(changes[0])
        self.assertFalse(changes[-2])
        # Duplicate evidence gets no additional vote, but deleting one copy must
        # preserve the remaining identity and its topic ID.
        def redis_tags(snapshot):
            return next(row['tags'] for row in snapshot['threads']
                        if row['thread'] == 'claude-thread')
        self.assertEqual(redis_tags(expected[1]), redis_tags(expected[2]))
        self.assertEqual(redis_tags(expected[2]), redis_tags(expected[4]))
        remaining = [t for t in expected[4]['topics']
                     if 'claude-thread' in t['threads']]
        self.assertTrue(remaining)
        for topic in remaining:
            self.assertEqual(topic['id'], redis_ids[topic['phrase']])
        new_tags = next(row['tags'] for row in expected[5]['threads']
                        if row['thread'] == 'new-thread')
        self.assertTrue(new_tags)
        self.assertTrue(all(tag['topic'] not in set(redis_ids.values())
                            for tag in new_tags))
        self.assertTrue(all('qdrant-new' in topic['sources']
                            for topic in expected[5]['topics']
                            if topic['id'] in {tag['topic'] for tag in new_tags}))
        self.assertIn('sqlite-original', expected[3]['memberships'])
        self.assertFalse(any('sqlite' in t['phrase'] for t in expected[3]['topics']))
        self.assertFalse(any('redis' in t['phrase'] for t in expected[-1]['topics']))
        self.assertNotIn('redis-original', expected[-1]['memberships'])
        self.assertNotIn('redis-copy', expected[-1]['memberships'])

    def test_invalid_disk_checkpoint_does_not_publish_a_snapshot(self):
        model = PhraseStore('node-group/reference-v1')
        model.upsert(PhraseSource('a', 'thread', 'claude', 'redis persistence'))
        checkpoint = model.checkpoint()
        checkpoint['records'].append(checkpoint['records'][0])
        with TemporaryDirectory() as temporary:
            result, output_path = self.run_worker(Path(temporary), checkpoint, [])
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output_path.exists())
