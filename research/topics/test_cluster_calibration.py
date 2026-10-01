from dataclasses import replace
import unittest

import numpy as np

from cluster_calibration import choose, evaluate_partition, plan, split
from external_clustering import fit
from multiwoz import Corpus, Dialogue, Label, Service, Speaker, Turn


class CalibrationTests(unittest.TestCase):
    def test_group_split_is_disjoint_and_label_independent(self):
        ds = tuple(Dialogue(str(i), f'{i:064x}', (Turn(str(i), 0, Speaker.USER, f'text {i}'),)) for i in range(80))
        labels = tuple(Label(str(i), 0, (Service.HOTEL,), (), ((Service.HOTEL, 'find_hotel'),)) for i in range(80))
        original, calibration, reserved = split(Corpus(ds, labels, (), ()))
        self.assertEqual([len(x) for x in (original, calibration, reserved)], [32, 16, 16])
        self.assertEqual(len({d.group for d in (*original, *calibration, *reserved)}), 64)
        changed = tuple(replace(l, active=(), declared_intents=()) for l in labels)
        self.assertEqual(split(Corpus(ds[::-1], changed, (), ())), (original, calibration, reserved))

    def test_model_plan_preserves_exact_text_and_root_abstention(self):
        d = Dialogue('d', 'g', (Turn('d', 0, Speaker.USER, 'Hello'), Turn('d', 1, Speaker.SYSTEM, 'Option?'),
                                Turn('d', 2, Speaker.USER, 'Yes.')))
        plans = plan([d])
        self.assertEqual(plans['d:0']['joined'], ())
        self.assertEqual(plans['d:2']['joined'], ('Option?\n\nYes.',))

    def test_selection_uses_calibration_f1_and_fixed_tie(self):
        candidates = {key: {'metrics': {'service': {'f1': value}}} for key, value in [('z', .8), ('a', .8), ('b', .7)]}
        self.assertEqual(choose(candidates, 'service'), 'a')
        candidates['a']['metrics']['service']['f1'] = None
        self.assertEqual(choose(candidates, 'service'), 'z')
        with self.assertRaises(ValueError):
            choose({}, 'service')

    def test_all_one_baseline_counts_false_positive_merges(self):
        rows = [{'left': 'a', 'right': 'b', 'service_overlap': True, 'intent_overlap': False},
                {'left': 'a', 'right': 'c', 'service_overlap': False, 'intent_overlap': False}]
        metrics = evaluate_partition([['a', 'b', 'c']], ['a', 'b', 'c'], rows)
        self.assertEqual(metrics['service']['precision'], .5)
        self.assertEqual(metrics['intent']['fp'], 2)
        self.assertEqual(metrics['within_service_intent']['fp'], 1)

    def test_thresholds_change_groups_and_reject_invalid_values(self):
        matrix = np.array([[1., 0.], [.5, np.sqrt(.75)]])
        for method in ('birch', 'grinch'):
            for invalid in (float('nan'), 0, 1.1):
                with self.assertRaises(ValueError):
                    fit(['a', 'b'], matrix, [0, 1], method, invalid)
        self.assertEqual(len(fit(['a', 'b'], matrix, [0, 1], 'birch', .3)[0]['clusters']), 2)
        self.assertEqual(len(fit(['a', 'b'], matrix, [0, 1], 'birch', .8)[0]['clusters']), 1)


if __name__ == '__main__':
    unittest.main()
