import unittest

from source_spans import Kind, structural_spans


class SourceSpanTests(unittest.TestCase):
    def test_unicode_crlf_offsets_preserve_exact_source(self):
        text = "  α🙂 first\r\nwrapped\r\n\r\n  second  \r\n"
        spans = structural_spans(text)
        self.assertEqual([text[s.start:s.end] for s in spans], ["α🙂 first\r\nwrapped", "second"])
        self.assertTrue(all(s.kind == Kind.PROSE for s in spans))

    def test_table_rows_are_separate_and_header_is_not_a_topic(self):
        text = "| Topic | Plan |\n|---|:---:|\n| memory | cache |\n| locks | proof |\n"
        spans = structural_spans(text)
        self.assertEqual([text[s.start:s.end] for s in spans], ["| memory | cache |", "| locks | proof |"])
        self.assertTrue(all(s.kind == Kind.TABLE for s in spans))

    def test_lists_headings_and_following_prose_do_not_overlap(self):
        text = "Intro\n\n# Heading\n- one\n2. two\nremaining prose\n"
        spans = structural_spans(text)
        self.assertEqual([s.kind for s in spans], [Kind.PROSE, Kind.HEADING, Kind.LIST, Kind.LIST, Kind.PROSE])
        self.assertTrue(all(a.end <= b.start for a, b in zip(spans, spans[1:])))

    def test_code_is_not_split_into_pseudo_table_or_list_topics(self):
        text = "```rust\n| a | b |\n|---|---|\n- fake list\n\n```\n\nreal prose"
        spans = structural_spans(text)
        self.assertEqual(len(spans), 2)
        self.assertEqual(text[spans[0].start:spans[0].end], text.split("\n\nreal prose")[0])
        self.assertEqual(spans[0].kind, Kind.CODE)

    def test_unclosed_fence_keeps_remainder_and_empty_text_has_no_units(self):
        text = "before\n\n~~~\n- still code\n"
        spans = structural_spans(text)
        self.assertEqual(len(spans), 2)
        self.assertEqual(text[spans[-1].start:spans[-1].end], "~~~\n- still code")
        self.assertEqual(structural_spans(" \n\t"), ())
