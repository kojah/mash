from dataclasses import replace
import unittest

from calibrate_boundaries import Row, evaluate, fold, threshold
from episodes import Judgment


def row(group, judgment, value):
    return Row(group, group, "codex", 0, 1, judgment, value)


class CalibrationTests(unittest.TestCase):
    def test_all_evaluation_group_aliases_are_excluded_from_training(self):
        rows = (row("train", Judgment.SAME, 0.1), row("train", Judgment.CHANGE, 0.9),
                row("test", Judgment.CHANGE, 0.2), replace(row("test", Judgment.SAME, 0.8), thread="alias"))
        cutoff, _ = fold(rows, "test")
        edited = (*rows[:2], replace(rows[2], judgment=Judgment.SAME, score=0.99),
                  replace(rows[3], judgment=Judgment.CHANGE, score=0.01))
        self.assertEqual(cutoff, fold(edited, "test")[0])
        self.assertEqual(cutoff, 0.5)

    def test_uncertain_and_missing_training_scores_never_choose_cutoffs(self):
        training = (row("a", Judgment.SAME, 0.1), row("a", Judgment.CHANGE, 0.9))
        extra = (row("b", Judgment.UNCERTAIN, 0.6), row("b", Judgment.CHANGE, None))
        self.assertEqual(threshold(training), threshold((*training, *extra)))
        self.assertIsNone(threshold((row("a", Judgment.SAME, 0.1),))[0])

    def test_exact_tie_prefers_fewer_splits(self):
        cutoff, details = threshold((row("a", Judgment.SAME, 0.5), row("a", Judgment.CHANGE, 0.5)))
        self.assertGreater(cutoff, 0.5)
        self.assertEqual(details["balanced_accuracy"], 0.5)

    def test_abstentions_do_not_earn_positive_or_negative_credit(self):
        result = evaluate((row("a", Judgment.SAME, None), row("a", Judgment.CHANGE, None)), [None, None])
        self.assertEqual(result["abstained"], 2)
        self.assertEqual(result["recall_including_abstentions"], 0)
        self.assertEqual(result["specificity_including_abstentions"], 0)
