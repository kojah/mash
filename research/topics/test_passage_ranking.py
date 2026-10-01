import unittest

from passage_ranking import counts, recency
from passage_scope import Judgment, Relevance, Scope


class PassageRankingTests(unittest.TestCase):
    def test_rejection_and_mixed_scope_retain_topicality(self):
        labels = [Judgment('t', 3, 1, i, relevance, scope) for i, (relevance, scope) in enumerate((
            (Relevance.TOPICAL, Scope.EXCLUDED), (Relevance.TOPICAL, Scope.MIXED),
            (Relevance.TOPICAL, Scope.INCLUDED), (Relevance.PROCEDURE, Scope.NA)))]
        self.assertEqual(counts([0, 1], labels), {'selected': 2, 'topical': 2, 'included': 0,
                         'excluded': 1, 'mixed': 1, 'unspecified': 0, 'non_topical': 0})
        self.assertEqual(counts([], labels)['selected'], 0)
        self.assertEqual(counts([2, 3], labels)['included'], 1)

    def test_recency_preserves_reading_order(self):
        self.assertEqual(recency([{'source_ordinal': 1, 'index': 0},
                                  {'source_ordinal': 2, 'index': 1},
                                  {'source_ordinal': 2, 'index': 0}]), (2, 1, 0))
