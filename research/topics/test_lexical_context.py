import unittest
import json

import numpy as np

from lexical_context import LexicalControl, background, fuse, lexical_features, serialized_policy
from external_context import compare
from multiwoz import Dialogue, Speaker, Turn


class LexicalContextTests(unittest.TestCase):
    def test_emitted_policy_survives_json_roundtrip_and_repeat_comparison(self):
        capture = {'timings': {'total_ms': 1}, 'policy': serialized_policy()}
        self.assertEqual(capture['policy']['ngram_range'], [1, 2])
        restored = json.loads(json.dumps(capture))
        restored['timings']['total_ms'] = 2
        compare(restored, capture)

    def test_background_excludes_whole_groups_exact_evaluation_text_and_duplicates(self):
        def dialogue(identity, group, user, system):
            return Dialogue(identity, group, (Turn(identity, 0, Speaker.USER, user), Turn(identity, 1, Speaker.SYSTEM, system)))
        source = (dialogue('eval', 'selected', 'book the hotel', 'annotation-like system content'),
                  dialogue('a', 'other-a', 'book the hotel', 'system text never becomes lexical fit'),
                  dialogue('b', 'other-b', 'find a train', 'hidden service description'),
                  dialogue('c', 'other-c', 'find a train', 'different hidden intent description'),
                  dialogue('d', 'other-d', 'reserve a restaurant', 'metadata'))
        texts, proof = background(source, {'selected'})
        self.assertEqual(set(texts), {'find a train', 'reserve a restaurant'})
        self.assertEqual(proof['source_user_turns'], 4)
        self.assertEqual(proof['exact_evaluation_text_exclusions'], 1)
        self.assertEqual(proof['duplicate_background_text_exclusions'], 1)
        with self.assertRaises(ValueError): background(source, set())
        with self.assertRaises(ValueError): background(source, {d.group for d in source})

    def test_eval_only_terms_cannot_enter_fit_and_negation_is_preserved(self):
        fitted, features, _ = lexical_features(['do not book hotel', 'do not book train', 'find hotel', 'find train'],
                                               ['uniquetermonly', 'not book'])
        vocabulary = set(fitted.get_feature_names_out())
        self.assertNotIn('uniquetermonly', vocabulary)
        self.assertIn('not', vocabulary)
        self.assertIn('not book', vocabulary)
        self.assertEqual(features[0].nnz, 0)
        self.assertGreater(features[1].nnz, 0)
        self.assertAlmostEqual(float((features[1] @ features[1].T).toarray()[0, 0]), 1.)

    def test_overlap_and_malformed_text_never_fit(self):
        for selected in (['book hotel'], [''], [True]):
            with self.assertRaises(ValueError): lexical_features(['book hotel', 'book train'], selected)

    def test_fixed_fusion_weights_and_invalid_cosines(self):
        self.assertEqual(fuse(1., -1., 0.5), {LexicalControl.LEXICAL: 0.5, LexicalControl.CURRENT: 0.875,
                                             LexicalControl.JOINED: -0.625})
        for bad in (np.nan, np.inf, 2., -2.):
            with self.assertRaises(ValueError): fuse(0., 0., bad)
