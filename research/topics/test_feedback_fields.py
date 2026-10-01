import unittest

from feedback_fields import Role, Selection, included, parse, records
from phrase_labels import source_terms


class FeedbackFieldTests(unittest.TestCase):
    def test_standard_fields_have_exact_unicode_coverage_and_separate_roles(self):
        text = '\n## Page Feedback: /architecture/\r\n**Viewport:** 900×500\r\n### 1. h3 "α🙂 heading"\r\n**Location:** .css\r\n**Feedback:** Do not remove cache\r\nkeep locks.\r\n'
        spans = parse(text)
        self.assertEqual(''.join(text[s.start:s.end] for s in spans), text)
        chosen = records(text, spans, Selection.TASK)
        self.assertIn('"α🙂 heading"\r\n', chosen)
        self.assertIn('Do not remove cache\r\nkeep locks.\r\n', chosen)
        self.assertFalse(any('/architecture/' in part or '.css' in part or '900' in part for part in chosen))

    def test_new_layout_free_text_and_unknown_fields_are_not_silently_removed(self):
        text = '## Newly submitted page feedback\n**Page:** /docs\n### 1. "Cache"\nMove this under Runtime\n**Element path:** .selector\n**Nearby text:** irrelevant\n**Unrecognized field:** important custom content\n'
        spans = parse(text)
        selected = records(text, spans, Selection.FEEDBACK)
        self.assertIn('Move this under Runtime\n', selected)
        self.assertIn('important custom content\n', selected)
        self.assertFalse(any('.selector' in part or '/docs' in part for part in selected))

    def test_code_field_markers_are_content_and_unknown_layout_abstains(self):
        text = '## Page Feedback: /docs\n### 1. "Parser"\n**Feedback:** Keep this example:\n```text\n**Location:** do not delete\n### 2. code, not a task\n```\n'
        spans = parse(text)
        self.assertTrue(any(s.role == Role.FEEDBACK and '**Location:**' in text[s.start:s.end] for s in spans))
        self.assertEqual(''.join(records(text, spans, Selection.FEEDBACK)), 'Keep this example:\n```text\n**Location:** do not delete\n### 2. code, not a task\n```\n')
        with self.assertRaises(ValueError):
            parse('ordinary conversation; not feedback schema')

    def test_filtered_fields_never_create_cross_field_phrases(self):
        text = '## Page Feedback: /docs\n### 1. "database"\n**Location:** .selector\n**Feedback:** timeout\n'
        parts = records(text, parse(text), Selection.TASK)
        terms = [t for part in parts for t in source_terms(part)]
        self.assertIn('database', terms)
        self.assertIn('timeout', terms)
        self.assertNotIn('database timeout', terms)
        self.assertTrue(included(Role.UNKNOWN, Selection.FEEDBACK))
