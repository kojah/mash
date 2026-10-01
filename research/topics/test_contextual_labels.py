import unittest

from contextual_labels import Mode, negation_veto, units
from reference_review import Resolution


class ContextualLabelTests(unittest.TestCase):
    def setUp(self):
        self.context = {"stop": "root", "display": [
            {"ordinal": 0, "role": "user", "text": "metadata frame", "display_status": "included"},
            {"ordinal": 1, "role": "assistant", "text": "concrete proposal", "display_status": "included"}]}

    def test_only_resolved_task_receives_source_context(self):
        for resolution in (Resolution.METADATA, Resolution.INTERACTION, Resolution.UNRESOLVED, None):
            self.assertEqual(units(2, "yes", "referential", resolution, self.context, Mode.CONTEXT, {0: "protocol"}), ())
        result = units(2, "yes", "referential", Resolution.TASK, self.context, Mode.CONTEXT, {0: "protocol"})
        self.assertEqual([u.ordinal for u in result], [1, 2])

    def test_content_retains_its_source_without_assistant_borrowing(self):
        result = units(2, "actual task", "content", None, self.context, Mode.CONTEXT, {0: "content"})
        self.assertEqual([u.text for u in result], ["actual task"])

    def test_partial_display_abstains_without_fallback(self):
        self.context["display"][1]["display_status"] = "omitted-size"
        self.assertEqual(units(2, "yes", "referential", Resolution.TASK, self.context, Mode.CONTEXT, {0: "content"}), ())

    def test_user_only_control_does_not_consult_resolution(self):
        for resolution in Resolution:
            result = units(2, "yes", "referential", resolution, self.context, Mode.ALL, {})
            self.assertEqual([u.text for u in result], ["yes"])

    def test_negation_guard_preserves_anchor_without_borrowing(self):
        result = units(2, "No, use the other project", "referential", Resolution.TASK,
                       self.context, Mode.GUARDED, {0: "content"})
        self.assertEqual([u.text for u in result], ["No, use the other project"])
        result = units(2, "proceed", "referential", Resolution.TASK,
                       self.context, Mode.GUARDED, {0: "content"})
        self.assertEqual([u.ordinal for u in result], [0, 1, 2])

    def test_guard_matches_words_and_contractions_not_identifier_substrings(self):
        for text in ("not that", "NEVER", "do this instead", "rather use that", "don't", "isn’t"):
            self.assertTrue(negation_veto(text))
        for text in ("NoSQL", "notebook", "Notion", "another option"):
            self.assertFalse(negation_veto(text))
