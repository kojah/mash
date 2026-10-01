from dataclasses import asdict
import json
import unittest

from projection_coverage import Embedding, GENERATION, Projection, classify


class CoverageTests(unittest.TestCase):
    def test_empty_override_is_not_missing_inference(self):
        state = classify('canonical tool output', '', None, None, None, None, False)
        self.assertEqual(state.projection, Projection.EMPTY)
        self.assertEqual(state.embedding, Embedding.EXCLUDED)
        self.assertFalse(state.sql_searchable)
        self.assertEqual(asdict(state), json.loads(json.dumps(asdict(state))))

    def test_null_override_uses_canonical_and_pending_is_independent(self):
        state = classify('request', None, None, None, None, None, True)
        self.assertEqual(state.projection, Projection.CANONICAL)
        self.assertEqual(state.embedding, Embedding.MISSING)
        self.assertTrue(state.pending)

    def test_sql_nonempty_differs_from_nonblank(self):
        state = classify('original', ' \n ', None, None, None, None, False)
        self.assertEqual(state.projection, Projection.WHITESPACE)
        self.assertTrue(state.sql_searchable)
        self.assertFalse(state.nonblank)

    def test_vector_generation_and_exclusion_are_checked(self):
        raw = bytes([1] + [0] * 383)
        self.assertEqual(classify('request', None, raw, GENERATION, 384, 1., False).embedding, Embedding.COMPATIBLE)
        self.assertEqual(classify('request', '', raw, GENERATION, 384, 1., False).embedding, Embedding.UNEXPECTED)
        for space, dimensions, norm in [('wrong', 384, 1.), (GENERATION, 383, 1.), (GENERATION, 384, 5.)]:
            self.assertEqual(classify('request', None, raw, space, dimensions, norm, False).embedding, Embedding.INCOMPATIBLE)

    def test_malformed_known_fields_are_rejected(self):
        for content, projection, pending in [(None, '', False), ('x', 1, False), ('x', None, 1)]:
            with self.assertRaises(ValueError):
                classify(content, projection, None, None, None, None, pending)


if __name__ == '__main__':
    unittest.main()
