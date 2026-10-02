import math
import unittest

import numpy as np

from lexical_retrieval import Method, ScoreKind, evaluate, fit


class LexicalRetrievalTests(unittest.TestCase):
    def test_bm25_complete_length_and_binary_query_formula(self):
        index = fit(['common common uniqueone', 'common rare rare', 'rare uniquetwo uniquethree'], Method.BM25)
        self.assertEqual(index.lengths.tolist(), [3, 3, 3])
        scores, evidence = index.score('common common')
        idf = math.log1p(1.5 / 2.5)
        np.testing.assert_allclose(scores, [idf * 2 * 2.2 / 3.2, idf, 0], atol=1e-12)
        self.assertEqual(evidence, 1)
        np.testing.assert_array_equal(scores, index.score('common')[0])

    def test_tfidf_smoothing_sublinear_norm_and_unknown_query(self):
        index = fit(['alpha alpha beta', 'alpha beta', 'beta gamma'], Method.WORD)
        scores, _ = index.score('alpha')
        idf_alpha = math.log(4 / 3) + 1
        a = (1 + math.log(2)) * idf_alpha
        self.assertAlmostEqual(scores[0], a / math.sqrt(a*a + 1))
        self.assertAlmostEqual(scores[1], idf_alpha / math.sqrt(idf_alpha**2 + 1))
        self.assertEqual(scores[2], 0)
        zero, evidence = index.score('unobserved')
        np.testing.assert_array_equal(zero, [0, 0, 0])
        self.assertEqual(evidence, 0)

    def test_character_preserves_identifiers_word_baseline_collision_explicit(self):
        texts = ['C++ language', 'C++ language', 'C# language', 'C# language']
        character = fit(texts, Method.CHARACTER)
        scores, _ = character.score('C++')
        self.assertGreater(scores[0], scores[2])
        words = fit(texts, Method.WORD)
        self.assertEqual(words.score('C++')[1], 0)

    def test_ranking_ties_and_metrics(self):
        result = evaluate([0, 1, 1, 0], ['a', 'b', 'c', 'd'], {'c', 'd'})
        self.assertEqual([r['id'] for r in result['top100']], ['b', 'c', 'a', 'd'])
        self.assertEqual(result['relevant_ranks'], {'c': 2, 'd': 4})
        self.assertEqual(result['metrics']['mrr10'], .5)
        self.assertEqual(result['metrics']['recall10'], 1)
        self.assertAlmostEqual(result['metrics']['ndcg10'],
                               (1/math.log2(3) + 1/math.log2(5)) / (1 + 1/math.log2(3)))
        zero = evaluate([0]*12, [f'{i:02}' for i in range(12)], {'11'})
        self.assertEqual(zero['metrics']['mrr10'], 0)
        self.assertEqual(zero['metrics']['recall100'], 1)

    def test_reject_incomplete_or_invalid_evidence(self):
        for scores, ids, refs in [([1], ['b','a'], {'a'}), ([math.nan], ['a'], {'a'}),
                                  ([-1], ['a'], {'a'}), ([1], ['a'], set()),
                                  ([1], ['a'], {'missing'}), ([1,1], ['a','a'], {'a'})]:
            with self.assertRaises(ValueError): evaluate(scores, ids, refs)
        for texts, method in [([], Method.WORD), (['x','x'], 'invalid'), (['x'*65538,'word'], Method.WORD)]:
            with self.assertRaises(ValueError): fit(texts, method)

    def test_signed_cosine_preserves_scores_order_and_strict_domain(self):
        result = evaluate([-.5, .2, -.1], ['a','b','c'], {'c'}, score_kind=ScoreKind.COSINE)
        self.assertEqual(result['first_relevant_rank'], 2)
        self.assertEqual([r['score'] for r in result['top100']], [.2,-.1,-.5])
        self.assertEqual(result['positive_score_documents'], 1)
        with self.assertRaises(ValueError):
            evaluate([-1.1], ['a'], {'a'}, score_kind=ScoreKind.COSINE)
        with self.assertRaises(ValueError):
            evaluate([.5], ['a'], {'a'}, score_kind='cosine')


if __name__ == '__main__':
    unittest.main()
