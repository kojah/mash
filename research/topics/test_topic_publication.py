"""Controlled futures test the publication contract without clock races."""
from concurrent.futures import Future, ProcessPoolExecutor
from copy import deepcopy
import multiprocessing
from unittest import TestCase

from phrase_selection import Selection
from topic_phrase_store import PhraseStore
from topic_phrases import PhraseSource
from topic_publication import Publisher, build_snapshot


class ControlledJobs:
    def __init__(self):
        self.calls = []

    def __call__(self, checkpoint):
        future = Future()
        self.calls.append((checkpoint, future))
        return future

    def finish(self):
        checkpoint, future = self.calls[-1]
        future.set_result(build_snapshot(checkpoint))


class PublicationTests(TestCase):
    def make(self):
        model = PhraseStore('group/provider-independent-v1')
        jobs = ControlledJobs()
        return model, jobs, Publisher(model, jobs)

    def test_coalesces_without_losing_edits_deletions_or_new_subjects(self):
        model, jobs, publisher = self.make()
        model.upsert(PhraseSource('a', 'claude-thread', 'claude', 'redis persistence'))
        publisher.poll()
        initial = deepcopy(jobs.calls[0][0])
        model.upsert(PhraseSource('b', 'codex-thread', 'codex', 'sqlite query planner'))
        model.upsert(PhraseSource('a', 'claude-thread', 'claude', 'postgres isolation'))
        model.delete('b')
        model.upsert(PhraseSource('c', 'new-thread', 'codex', 'qdrant vector filtering'))
        for _ in range(20):
            self.assertIsNone(publisher.poll())
            self.assertEqual((publisher.jobs, publisher.pending), (1, 1))
        self.assertEqual(len(jobs.calls), 1)
        self.assertEqual(jobs.calls[0][0], initial)
        jobs.finish()
        result = publisher.poll()
        self.assertEqual(result.snapshot['revision'], 1)
        self.assertEqual(len(jobs.calls), 2)
        self.assertEqual(jobs.calls[1][0], model.checkpoint())
        self.assertFalse(publisher.drained)
        jobs.finish()
        final = publisher.poll()
        self.assertEqual(final.snapshot, model.snapshot(Selection.SOURCE_COVERAGE))
        self.assertEqual(final.snapshot['revision'], 5)
        self.assertEqual(set(final.snapshot['memberships']), {'a', 'c'})
        self.assertFalse(any('redis' in row['phrase'] for row in final.snapshot['topics']))
        self.assertTrue(next(row['tags'] for row in final.snapshot['threads']
                             if row['thread'] == 'new-thread'))
        self.assertTrue(publisher.drained)
        self.assertEqual((publisher.jobs, publisher.pending), (0, 0))

    def test_duplicates_and_missing_deletes_do_not_queue_work(self):
        model, jobs, publisher = self.make()
        source = PhraseSource('a', 't', 'p', 'sqlite indexes')
        model.upsert(source)
        publisher.poll()
        jobs.finish()
        publisher.poll()
        self.assertFalse(model.upsert(source))
        self.assertFalse(model.delete('missing'))
        publisher.poll()
        self.assertEqual(len(jobs.calls), 1)
        self.assertTrue(publisher.drained)

    def test_failed_refresh_retains_publication_and_requires_explicit_retry(self):
        model, jobs, publisher = self.make()
        publisher.poll()
        jobs.finish()
        previous = publisher.poll()
        model.upsert(PhraseSource('a', 't', 'p', 'redis persistence'))
        publisher.poll()
        jobs.calls[-1][1].set_exception(RuntimeError('refresh failed'))
        with self.assertRaisesRegex(RuntimeError, 'refresh failed'):
            publisher.poll()
        self.assertIs(publisher.published, previous)
        self.assertTrue(publisher.failed)
        self.assertFalse(publisher.drained)
        publisher.poll()
        self.assertEqual(len(jobs.calls), 2)
        publisher.retry()
        jobs.finish()
        publisher.poll()
        self.assertTrue(publisher.drained)
        with self.assertRaises(ValueError):
            publisher.retry()

    def test_wrong_snapshot_revision_or_scope_never_replaces_publication(self):
        for field, value in [('revision', 123), ('namespace', 'other-group')]:
            model, jobs, publisher = self.make()
            publisher.poll()
            checkpoint, future = jobs.calls[-1]
            result = build_snapshot(checkpoint)
            result.snapshot[field] = value
            future.set_result(result)
            with self.assertRaises(ValueError):
                publisher.poll()
            self.assertIsNone(publisher.published)
            self.assertTrue(publisher.failed)

    def test_submission_failure_is_terminal_until_retry(self):
        model, jobs, publisher = self.make()
        def fail(_):
            raise RuntimeError('executor unavailable')
        publisher.submit = fail
        with self.assertRaises(RuntimeError):
            publisher.poll()
        self.assertTrue(publisher.failed)
        self.assertEqual(publisher.jobs, 0)
        publisher.submit = jobs
        publisher.retry()
        jobs.finish()
        publisher.poll()
        self.assertTrue(publisher.drained)

    def test_actual_spawned_worker_returns_identical_reference_snapshot(self):
        model = PhraseStore('group')
        model.upsert(PhraseSource('a', 't', 'p', 'redis recovery'))
        with ProcessPoolExecutor(max_workers=1,
                                 mp_context=multiprocessing.get_context('spawn')) as pool:
            publisher = Publisher(model, lambda checkpoint: pool.submit(build_snapshot, checkpoint))
            publisher.poll()
            # A timeout here is a test failure, never a signal to restart work.
            publisher._future.result(timeout=30)
            result = publisher.poll()
        self.assertEqual(result.snapshot, model.snapshot(Selection.SOURCE_COVERAGE))
        self.assertTrue(publisher.drained)
