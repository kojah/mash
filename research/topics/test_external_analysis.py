import copy
import unittest

import numpy as np

from external_analysis import evaluation_grid, pair_weights, prepare, summarize, weighted_metrics
from external_context import Control, SEED, inputs
from multiwoz import Corpus, Dialogue, Label, PINS, Service, Speaker, Turn, select
from reference_review import digest


def anchors():
    return {'a': {'group': 'one', 'active': ['hotel'], 'unknown': [], 'intents': [['hotel', 'find_hotel']]},
            'b': {'group': 'two', 'active': ['hotel'], 'unknown': [], 'intents': [['hotel', 'book_hotel']]},
            'c': {'group': 'three', 'active': ['hotel'], 'unknown': [], 'intents': [['hotel', 'find_hotel']]},
            'd': {'group': 'four', 'active': ['train'], 'unknown': [], 'intents': [['train', 'find_train']]}}


class ExternalAnalysisTests(unittest.TestCase):
    def test_frozen_capture_revalidation_rejects_source_and_vector_drift(self):
        dialogues, labels = [], []
        for n in range(32):
            identity = f'd{n}'
            turns = (Turn(identity, 0, Speaker.USER, f'root {n}'), Turn(identity, 1, Speaker.SYSTEM, f'reply {n}'),
                     Turn(identity, 2, Speaker.USER, f'next {n}'), Turn(identity, 3, Speaker.SYSTEM, 'done'))
            dialogues.append(Dialogue(identity, f'group{n}', turns))
            labels.extend(Label(identity, ordinal, (Service.HOTEL,), (), ((Service.HOTEL, 'find_hotel'),))
                          for ordinal in (0, 2))
        corpus = Corpus(tuple(dialogues), tuple(labels), (), ())
        selected, _ = select(corpus, SEED)
        capture = {'source_pins': PINS, 'seed': SEED, 'source': {'duplicates': 0},
                   'selected_groups': [{'id': d.id, 'group': d.group} for d in selected],
                   'anchors': {}, 'common_support': 32, 'results': {c: {'records': {}} for c in Control}}
        for dialogue in selected:
            for ordinal in (0, 2):
                turn = dialogue.turns[ordinal]
                identity = f'{dialogue.id}:{ordinal}'
                capture['anchors'][identity] = {'group': dialogue.group, 'active': ['hotel'], 'unknown': [],
                                                 'text_sha256': digest(turn.text.encode())}
                for control in Control:
                    texts = inputs(turn, dialogue.turns[ordinal - 1] if ordinal else None, control)
                    capture['results'][control]['records'][identity] = {
                        'reason': None if texts else 'root-context', 'input_hashes': [digest(t.encode()) for t in texts],
                        'vector': [1.] + [0.] * 383 if texts else None}
        self.assertEqual(len(prepare(corpus, capture)[2]), 32)
        key = f'{selected[0].id}:2'
        for mutation in ('source', 'input', 'reason', 'vector', 'coverage'):
            changed = copy.deepcopy(capture)
            record = changed['results']['joined']['records'][key]
            if mutation == 'source': changed['anchors'][key]['active'] = ['train']
            elif mutation == 'input': record['input_hashes'] = ['fabricated']
            elif mutation == 'reason': record['reason'] = 'root-context'
            elif mutation == 'vector': record['vector'][0] = 2.
            else: changed['results']['joined']['records']['extra'] = record
            with self.assertRaises(ValueError, msg=mutation): prepare(corpus, changed)

    def test_same_service_is_not_same_intent(self):
        source = anchors()
        vectors = {c: {key: np.array([1., 0.]) for key in source} for c in Control}
        rows = evaluation_grid(source, vectors, set(source))
        self.assertEqual(len(rows), 6)
        self.assertEqual(sum(r['service_overlap'] for r in rows), 3)
        self.assertEqual(sum(r['intent_overlap'] for r in rows), 1)
        within = [r for r in rows if r['service_overlap']]
        self.assertEqual((len(within), sum(r['intent_overlap'] for r in within)), (3, 1))

    def test_bootstrap_weights_are_group_products_and_no_self_pairs(self):
        source = anchors()
        source['b']['group'] = 'one'
        vectors = {c: {key: np.array([1., 0.]) for key in source} for c in Control}
        rows = evaluation_grid(source, vectors, set(source))
        self.assertFalse(any(r['left'] == 'a' and r['right'] == 'b' for r in rows))
        weights = pair_weights(rows, {'one': 2, 'three': 3, 'four': 0})
        self.assertEqual(weights.tolist(), [6, 0, 6, 0, 0])
        with self.assertRaises(ValueError): pair_weights(rows, {'one': -1})

    def test_weighted_ties_class_absence_and_perfect_ranking(self):
        self.assertEqual(weighted_metrics([True, False], [1., 0.], [3, 2]), {'ap': 1., 'auc': 1.})
        tied = weighted_metrics([True, False], [0., 0.], [3, 2])
        self.assertAlmostEqual(tied['ap'], 0.6)
        self.assertEqual(tied['auc'], 0.5)
        self.assertIsNone(weighted_metrics([True, False], [1., 0.], [1, 0]))
        with self.assertRaises(ValueError): weighted_metrics([True], [1.], [-1])

    def test_identical_controls_have_zero_paired_deltas_and_skips_reported(self):
        source = anchors()
        vectors = {c: {key: np.array([1., 0.]) for key in source} for c in Control}
        rows = evaluation_grid(source, vectors, set(source))
        draws = [{g: 1 for g in ('one', 'two', 'three', 'four')},
                 {'one': 4, 'two': 0, 'three': 0, 'four': 0}]
        result = summarize(rows, 'intent_overlap', draws)
        self.assertEqual(result['one_class_replicates'], 1)
        self.assertEqual(result['paired_delta_percentiles_2_5_50_97_5']['joined']['ap'], [0., 0., 0.])
