import unittest

from feedback_fields import Selection, parse
from field_labels import Score, label
from pair_review import Key


class FieldLabelTests(unittest.TestCase):
    def test_field_filter_keeps_groups_exact_and_never_invents_compounds(self):
        key = Key('t', 0, 0)
        text = '## Page Feedback: /docs\n### 1. "database"\n**Viewport:** 900 500\n**Location:** .selector\n**Feedback:** timeout\n'
        units = {key: text}
        parsed = {key: parse(text)}
        all_labels = label(units, [(key,)], parsed, Selection.ALL)
        filtered = label(units, [(key,)], parsed, Selection.TASK)
        self.assertEqual(all_labels['clusters'][0]['members'], filtered['clusters'][0]['members'])
        names = filtered['clusters'][0]['labels'][Score.CTFIDF]
        self.assertEqual({n['term'] for n in names}, {'database', 'timeout'})
        self.assertTrue(all(not n['metadata_framing_only'] for n in names))
        feedback = label(units, [(key,)], parsed, Selection.FEEDBACK)
        self.assertEqual([n['term'] for n in feedback['clusters'][0]['labels'][Score.CTFIDF]], ['timeout'])

    def test_code_in_feedback_is_not_silently_classified_as_location(self):
        key = Key('t', 0, 0)
        text = '## Page Feedback: /docs\n### 1. "Example"\n**Feedback:** preserve\n```\n**Location:** important cache\n```\n'
        result = label({key: text}, [(key,)], {key: parse(text)}, Selection.FEEDBACK)
        names = result['clusters'][0]['labels'][Score.CTFIDF]
        self.assertIn('important cache', {n['term'] for n in names})
        self.assertTrue(all(not n['metadata_framing_only'] for n in names))
