import unittest

import numpy as np

from subject_labels import label_scores, ranked_terms


class SubjectLabelTests(unittest.TestCase):
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
