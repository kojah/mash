import unittest

import numpy as np

from subject_labels import label_scores, ranked_terms


class SubjectLabelTests(unittest.TestCase):
    def test_bounded_selection_matches_full_sort_on_dense_sparse_and_tied_scores(self):
        rng = np.random.default_rng(17)
        for size in (0, 1, 7, 13, 100, 8192):
            terms = [f'word-{i:05}' for i in rng.permutation(size)]
            for sparsity in (0., .5, .99, 1.):
                values = rng.integers(0, 5, size=size).astype(float)
                values[rng.random(size) < sparsity] = 0
                for limit in (1, 8, size + 1):
                    full = sorted(range(size), key=lambda i: (-values[i], terms[i]))
                    expected = [{'term': terms[i], 'score': float(values[i])}
                                for i in full if values[i] > 0][:limit]
                    self.assertEqual(ranked_terms(values, terms, limit=limit), expected)

    def test_diversity_scans_beyond_initial_top_k_and_keeps_lexical_ties(self):
        terms = ['a b', 'a', 'b', 'c', 'd', 'c d', 'e']
        values = np.array([5., 4., 4., 3., 3., 2., 1.])
        self.assertEqual([r['term'] for r in ranked_terms(values, terms, diverse=True, limit=4)],
                         ['a b', 'c', 'd', 'e'])
        for order in (list(reversed(range(7))), [6, 2, 0, 5, 1, 4, 3]):
            self.assertEqual(ranked_terms(values[order], [terms[i] for i in order], diverse=True),
                             ranked_terms(values, terms, diverse=True))

    def test_zero_scores_large_budget_and_duplicate_vocabulary_keep_contract(self):
        self.assertEqual(ranked_terms(np.array([0., -0., 2.]), ['a', 'b', 'c'], limit=100),
                         [{'term': 'c', 'score': 2.}])
        with self.assertRaises(ValueError):
            ranked_terms(np.array([1., 2.]), ['a', 'a'])

    def test_background_frequency_demotes_common_term_without_erasing_rare_one(self):
        counts = np.array([[1, 1], [1, 1]])
        background = np.array([[1, 1], [1, 0], [1, 0]])
        baseline, adjusted = label_scores(counts, background)
        self.assertEqual(baseline[0, 0], baseline[0, 1])
        self.assertGreater(adjusted[0, 1], adjusted[0, 0])

    def test_diversity_removes_contained_terms_but_keeps_distinct_compounds(self):
        terms = ["model phase", "model", "model memory", "other"]
        ranked = ranked_terms(np.array([4., 3., 2., 1.]), terms, diverse=True)
        self.assertEqual([r["term"] for r in ranked], ["model phase", "model memory", "other"])

    def test_tie_order_is_invariant_to_vocabulary_permutation(self):
        a = ranked_terms(np.array([1., 1.]), ["z", "a"])
        b = ranked_terms(np.array([1., 1.]), ["a", "z"])
        self.assertEqual(a, b)

    def test_empty_subject_does_not_create_keywords(self):
        baseline, adjusted = label_scores(np.zeros((2, 3)), np.zeros((1, 3)))
        self.assertEqual(ranked_terms(adjusted[0], ["a", "b", "c"]), [])
        self.assertTrue(np.isfinite(baseline).all())

    def test_invalid_counts_or_ranking_scores_fail_closed(self):
        with self.assertRaises(ValueError):
            label_scores(np.array([[-1.]]), np.ones((1, 1)))
        for values, limit in (([float("nan")], 8), ([1.], 0)):
            with self.assertRaises(ValueError):
                ranked_terms(np.array(values), ["term"], limit=limit)
