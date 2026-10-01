from pathlib import Path
import tempfile
import unittest

from question_transfer import contract, partition_metrics, question_pairs, rank_metrics


class TransferTests(unittest.TestCase):
    def test_sparse_tag_overlap_and_all_connected_are_explicit(self):
        rows = question_pairs(['a', 'b', 'c'], {'a': ('rust',), 'b': ('rust', 'ffi'), 'c': ('python',)})
        self.assertEqual([r['tag_overlap'] for r in rows], [True, False, False])
        connected = partition_metrics(rows, dict.fromkeys(['a', 'b', 'c'], 0))
        self.assertEqual(connected['precision'], 1 / 3)
        self.assertEqual(connected['specificity'], 0.)
        good = partition_metrics(rows, {'a': 0, 'b': 0, 'c': 1})
        self.assertEqual(good['precision'], 1.)
        self.assertEqual(good['specificity'], 1.)
        self.assertEqual(rank_metrics(rows, [.9, .1, .1])['average_precision'], 1.)

    def test_no_positive_or_negative_proxy_is_reported_without_resampling(self):
        none = rank_metrics([{'tag_overlap': False}], [.1])
        self.assertIsNone(none['average_precision'])
        self.assertIsNone(none['roc_auc'])
        positive = rank_metrics([{'tag_overlap': True}], [.1])
        self.assertEqual(positive['average_precision'], 1.)
        self.assertIsNone(positive['roc_auc'])
        for rows, scores in [([{'tag_overlap': True}], []), ([{'tag_overlap': True}], [float('nan')]),
                             ([{'tag_overlap': 'true'}], [.1])]:
            with self.assertRaises(ValueError):
                rank_metrics(rows, scores)

    def test_wrong_calibration_hash_fails_before_parsing_or_inference(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / 'bad.json'
            path.write_text('not even JSON')
            with self.assertRaisesRegex(ValueError, 'calibration hash required'):
                contract(path)


if __name__ == '__main__':
    unittest.main()
