import unittest

from external_clustering import boolean_metrics
from multiwoz import Corpus, Dialogue, Label, Service, Speaker, Turn
from precision_calibration import choose, fresh_split, target_pass, thread_metrics


def metric(tp, fp, fn):
    return boolean_metrics([True] * tp + [False] * fp + [True] * fn,
                           [True] * (tp + fp) + [False] * fn)


class PrecisionTests(unittest.TestCase):
    def test_fresh_split_excludes_all_previous_groups(self):
        ds = tuple(Dialogue(str(i), f'{i:064x}', (Turn(str(i), 0, Speaker.USER, f'text {i}'),)) for i in range(112))
        labels = tuple(Label(str(i), 0, (Service.HOTEL,), (), ((Service.HOTEL, 'find_hotel'),)) for i in range(112))
        old, cal, reserved = fresh_split(Corpus(ds, labels, (), ()))
        self.assertEqual([len(x) for x in (old, cal, reserved)], [64, 16, 16])
        self.assertEqual(len({d.group for d in (*old, *cal, *reserved)}), 96)

    def test_precision_and_minimum_links_prevent_degenerate_selection(self):
        results = {key: {'candidate': {'linkage': 'average'}, 'metrics': {'intent': m}} for key, m in
                   [('good', metric(16, 4, 16)), ('bad-high-recall', metric(20, 20, 0)),
                    ('too-small', metric(10, 0, 0)), ('z', metric(20, 0, 20)), ('a', metric(20, 0, 20))]}
        self.assertEqual(choose(results, 'average', 'intent'), 'a')
        self.assertIsNone(choose(results, 'centroid', 'intent'))
        self.assertIsNone(choose({}, 'average', 'intent'))

    def test_thread_has_multiple_tags_without_gold_controlling_tags(self):
        def anchor(group, service, intent, unknown=False):
            return {'group': group, 'active': [service] if not unknown else [],
                    'intents': [[service, intent]] if not unknown else [], 'unknown': [[service, intent]] if unknown else []}
        anchors = {'a1': anchor('a', 'hotel', 'find'), 'a2': anchor('a', 'train', 'find'),
                   'b1': anchor('b', 'hotel', 'find'), 'c1': anchor('c', 'train', 'book'),
                   'd1': anchor('d', 'taxi', 'unknown', True)}
        result = thread_metrics([['a1', 'b1', 'd1'], ['a2', 'c1']], list(anchors), anchors, ['a', 'b', 'c', 'd', 'e'])
        self.assertEqual(len(result['direct_tags']['a']), 2)
        self.assertEqual(result['no_eligible_proxy_groups'], ['d', 'e'])
        self.assertEqual(len(result['direct_tags']['d']), 1)
        self.assertEqual(result['metrics']['service']['precision'], 1.)
        self.assertEqual(result['metrics']['intent']['precision'], .5)
        self.assertEqual(result['metrics']['intent']['recall'], 1.)

    def test_target_boundaries_and_no_predictions(self):
        self.assertTrue(target_pass(metric(14, 6, 10)))
        self.assertFalse(target_pass(metric(13, 7, 10)))
        self.assertFalse(target_pass(metric(19, 0, 10)))
        self.assertFalse(target_pass(metric(0, 0, 10)))

    def test_boolean_metric_boundary_rejects_coercions_and_missing_predictions(self):
        for truth, predicted in (([True], []), (['true'], [True]), ([True], [1])):
            with self.assertRaises(ValueError):
                boolean_metrics(truth, predicted)


if __name__ == '__main__':
    unittest.main()
