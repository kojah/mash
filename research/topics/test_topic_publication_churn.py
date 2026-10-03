"""Scheduler diagnostics must not credit coalesced events as independently visible."""
from unittest import TestCase
from topic_phrases import PhraseSource
from topic_publication_churn import Mutation, churn, represented, verify


class ChurnTests(TestCase):
    def test_original_source_pairs_wrap_and_restore_complete_cohort(self):
        sources = [PhraseSource(str(i), 'thread', 'codex', 'original') for i in range(3)]
        events = list(churn(sources, 10))
        self.assertEqual([row[0] for row in events], ['delete', 'upsert'] * 5)
        self.assertEqual([row[1].id for row in events[::2]], ['0', '1', '2', '0', '1'])
        for index in range(0, len(events), 2):
            self.assertIs(events[index][1], events[index + 1][1])
        for cohort, count in [([], 2), (sources, 3), (sources, True), (sources, -2)]:
            with self.assertRaises(ValueError):
                list(churn(cohort, count))

    def test_deleted_then_reinserted_transient_event_is_superseded(self):
        deleted = Mutation('delete', 'a', 1, 0., 0)
        inserted = Mutation('upsert', 'a', 2, 0., 0)
        latest = {'a': ('upsert', 2)}
        self.assertFalse(represented(deleted, latest, {'a': 2}))
        self.assertTrue(represented(inserted, latest, {'a': 2}))
        with self.assertRaises(ValueError):
            represented(inserted, latest, {'a': 1})

    def test_latest_deletion_requires_absence_and_unknown_actions_fail(self):
        event = Mutation('delete', 'a', 3, 0., 0)
        self.assertTrue(represented(event, {'a': ('delete', 3)}, {}))
        with self.assertRaises(ValueError):
            represented(event, {'a': ('delete', 3)}, {'a': 2})
        with self.assertRaises(ValueError):
            represented(Mutation('bad', 'a', 3, 0., 0), {'a': ('bad', 3)}, {})

    def test_each_captured_revision_scope_and_full_version_map_are_audited(self):
        snapshot = {'revision': 7, 'namespace': 'group', 'versions': {'a': 7}, 'memberships': {'a': []}}
        verify(snapshot, 7, 'group', {'a': 7})
        for revision, namespace, versions in [(8, 'group', {'a': 7}), (7, 'other', {'a': 7}), (7, 'group', {'a': 6}), (7, 'group', {})]:
            with self.assertRaises(ValueError):
                verify(snapshot, revision, namespace, versions)
        with self.assertRaises(ValueError):
            verify({**snapshot, 'memberships': {'missing': []}}, 7, 'group', {'a': 7})
