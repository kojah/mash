import unittest
import numpy as np

from record_selection import Record, eligible, hits, query_records, rank
from span_selection import Query


class RecordSelectionTests(unittest.TestCase):
    def test_recency_does_not_need_vector_but_cosine_requires_full_source_window(self):
        query = Record('q', 5, 5, np.array([1., 0.]))
        long = Record('long', 4, 513, np.array([1., 0.]))
        short = Record('short', 1, 512, np.array([0., 1.]))
        self.assertFalse(eligible(long))
        self.assertEqual(rank([short, long])[0]['id'], 'long')
        self.assertEqual(rank([long], [query]), [])
        self.assertEqual(rank([short], [query])[0]['cosine'], 0)
        self.assertEqual(rank([Record('missing', 2, 5, None)], [query]), [])

    def test_cosine_ties_use_source_ordinal_then_identity_and_canceling_queries_abstain(self):
        q = Record('q', 5, 5, np.array([1., 0.]))
        candidates = [Record('b', 2, 5, q.vector), Record('z', 1, 5, q.vector), Record('a', 2, 5, q.vector)]
        ranked = rank(candidates, [q])
        self.assertEqual([r['id'] for r in ranked], ['z', 'a', 'b'])
        self.assertEqual(hits(ranked, {'b'}), {'hit_at_1': False, 'hit_at_3': True})
        self.assertEqual(rank(candidates, [q, Record('negative', 6, 5, -q.vector)]), [])

    def test_missing_redacted_or_omitted_preceding_user_cannot_be_recovered_from_private_vector(self):
        q = Record('q', 5, 5, np.array([1., 0.]))
        u = Record('u', 0, 5, q.vector)
        records, anchor = {'q': q, 'u': u}, {'id': 'q'}
        self.assertEqual(query_records(anchor, {'display': []}, records, Query.CHAIN), [])
        for status in ('redacted', 'omitted-size'):
            context = {'display': [{'id': 'u', 'role': 'user', 'display_status': status}]}
            self.assertEqual(query_records(anchor, context, records, Query.PREVIOUS), [])
        context = {'display': [{'id': 'u', 'role': 'user', 'display_status': 'included'}]}
        self.assertEqual([r.id for r in query_records(anchor, context, records, Query.CHAIN)], ['u', 'q'])
