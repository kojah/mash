import unittest

from clustering import Membership, Result
from evaluation import Pair, Verdict, evaluate_pairs


class PairEvaluationContracts(unittest.TestCase):
    def test_overlap_ambiguous_and_unjudged(self) -> None:
        result = Result(
            (0, 0, 1, 2),
            (),
            (
                (Membership(0, 1.0),),
                (Membership(0, 1.0), Membership(1, 0.9)),
                (Membership(1, 1.0),),
                (),
            ),
        )
        pairs = (
            Pair(0, 1, Verdict.RELATED),
            Pair(1, 2, Verdict.UNRELATED),
            Pair(0, 3, Verdict.RELATED),
            Pair(0, 2, Verdict.AMBIGUOUS),
        )
        metrics = evaluate_pairs(result, pairs)
        self.assertEqual(metrics.true_positive, 1)
        self.assertEqual(metrics.false_positive, 1)
        self.assertEqual(metrics.false_negative, 1)
        self.assertEqual(metrics.true_negative, 0)
        self.assertEqual(metrics.ambiguous, 1)
        self.assertEqual(metrics.f1, 0.5)

    def test_empty_predictions_cannot_pass(self) -> None:
        result = Result((0, 1), (), ((), ()))
        metrics = evaluate_pairs(result, (Pair(0, 1, Verdict.RELATED),))
        self.assertEqual(metrics.recall, 0)
        self.assertEqual(metrics.f1, 0)

    def test_invalid_or_duplicate_pairs_fail(self) -> None:
        result = Result((0, 1), (), ((), ()))
        for pairs in (
            (Pair(0, 0, Verdict.RELATED),),
            (Pair(-1, 1, Verdict.RELATED),),
            (Pair(0, 2, Verdict.RELATED),),
            (Pair(0, 1, Verdict.RELATED), Pair(1, 0, Verdict.RELATED)),
        ):
            with self.assertRaises(ValueError):
                evaluate_pairs(result, pairs)


if __name__ == "__main__":
    unittest.main()
