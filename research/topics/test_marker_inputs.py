import unittest
from marker_inputs import Readiness,mask,readiness


class MarkerInputsTests(unittest.TestCase):
    def test_unicode_source_coordinates_and_surrounding_words_preserved(self):
        source='é database{disfmarker}timeout 😀';masked,ledger=mask(source,7)
        self.assertEqual(masked,'é database'+' '*12+'timeout 😀')
        self.assertEqual(len(masked),len(source));self.assertEqual(len(masked.encode()),len(source.encode()))
        span=ledger[0];self.assertEqual(span['source_row'],7)
        self.assertEqual(source[span['char_start']:span['char_end']],'{disfmarker}')
        self.assertEqual(source.encode()[span['utf8_start']:span['utf8_end']],b'{disfmarker}')
        self.assertEqual(masked[span['char_end']:],source[span['char_end']:])

    def test_plain_and_case_variants_unchanged_and_multiple_exact_spans_retained(self):
        source='disfmarker {Disfmarker} {vocalsound}{disfmarker}'
        masked,spans=mask(source,0);self.assertEqual(len(spans),2)
        self.assertTrue(masked.startswith('disfmarker {Disfmarker} '))

    def test_empty_marker_only_and_window_equality_have_explicit_status(self):
        self.assertEqual(readiness(mask('{disfmarker}',0)[0],2),Readiness.EMPTY)
        self.assertEqual(readiness('hello',512),Readiness.READY)
        self.assertEqual(readiness('hello',513),Readiness.WINDOW)
        with self.assertRaises(ValueError):readiness('hello',1)
