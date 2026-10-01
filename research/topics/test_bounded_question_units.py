import unittest
import json

from bounded_question_units import Method, Reason, capture_plan, plan
from question_html import render


class UnitTests(unittest.TestCase):
    def test_capture_round_trip_keeps_schema_and_complete_offsets(self):
        body = render('<p>é🦀 alpha beta gamma delta.</p>')
        value = capture_plan(plan('Q', body, len, 12))
        self.assertEqual(value, json.loads(json.dumps(value)))
        self.assertIsInstance(value['units'], list)
        self.assertTrue(all(isinstance(u['piece_indices'], list) for u in value['units']))
        self.assertEqual(value['units'][-1]['byte_end'], len(body.text.encode()))

    def assert_coverage(self, body, result):
        self.assertEqual(''.join(body.text[u.start:u.end] for u in result.units), body.text)
        for u in result.units:
            self.assertEqual(body.text[u.start:u.end].encode(), body.text.encode()[u.byte_start:u.byte_end])

    def test_whole_supported_input_retains_exact_original_cache_identity(self):
        body = render('<p>Use rust.</p>')
        result = plan('Question', body, len, 64)
        self.assertIsNone(result.reason)
        self.assertEqual(len(result.units), 1)
        self.assertEqual(result.units[0].method, Method.WHOLE)
        self.assert_coverage(body, result)

    def test_oversized_prose_and_code_keep_complete_ranges(self):
        body = render('<p>alpha beta gamma delta epsilon.</p><pre>line1\nline2\nline3</pre>')
        result = plan('Q', body, len, 18)
        self.assertIsNone(result.reason)
        self.assertGreater(len(result.units), 2)
        self.assert_coverage(body, result)
        self.assertTrue(all(u.reason is not None or u.tokens <= 18 for u in result.units))

    def test_unicode_codepoint_fallback_preserves_every_utf8_character(self):
        body = render('<pre>' + 'é🦀e\u0301' * 4 + '</pre>')
        result = plan('Q', body, len, 9)
        self.assert_coverage(body, result)
        self.assertTrue(any(u.method == Method.CODEPOINT for u in result.units))

    def test_count_cap_rejects_whole_question_without_losing_plan(self):
        body = render('<p>alpha beta gamma delta epsilon zeta eta theta.</p>')
        result = plan('Q', body, len, 12, 2)
        self.assertEqual(result.reason, Reason.COUNT)
        self.assert_coverage(body, result)
        self.assertGreater(len(result.units), 2)

    def test_whitespace_and_unsplittable_inputs_are_explicit(self):
        empty = plan('Q', render(' \n '), len)
        self.assertEqual(empty.reason, Reason.EMPTY)
        self.assertEqual(empty.units[0].reason, Reason.WHITESPACE)
        failed = plan('Q', render('x'), lambda _: 999)
        self.assertEqual(failed.reason, Reason.WINDOW)
        for maximum in [True, 1]:
            with self.assertRaises(ValueError):
                plan('Q', render('x'), len, maximum)

    def test_nonmonotonic_token_counts_are_checked_without_binary_search(self):
        body = render('<p>some text longer than the artificial tokenizer limit</p>')
        result = plan('Q', body, lambda text: 2 if len(text) > 30 else len(text), 20)
        self.assertEqual(len(result.units), 1)
        self.assertIsNone(result.reason)
        with self.assertRaises(ValueError):
            plan('Q', body, lambda _: True)


if __name__ == '__main__':
    unittest.main()
