import unittest

from preceding_context import Message, Role, preceding


def message(ordinal, role, text, parent=None):
    return Message(str(ordinal), ordinal, Role(role), parent, text, text)


class PrecedingTests(unittest.TestCase):
    def test_visible_budget_does_not_count_tools_or_empty_assistant_records(self):
        records = (message(0, 'user', 'task'), message(1, 'assistant', 'proposal'),
                   message(2, 'tool', 'output'), message(3, 'assistant', ''),
                   message(4, 'tool', 'output'), message(5, 'user', 'yes'))
        result = preceding(records, 5, maximum_users=3, maximum_visible=2)
        self.assertEqual([m['ordinal'] for m in result['display']], [0, 1])
        self.assertEqual(len(result['traversed']), 5)
        self.assertEqual(result['stop'], 'visible-record-limit')
        self.assertFalse(result['bounded_interval_complete'])
        capped = preceding(records, 5, maximum_records=2, maximum_visible=2)
        self.assertEqual(capped['stop'], 'record-limit')

    def test_omitted_visible_records_still_consume_budget(self):
        records = (message(0, 'user', 'task'), message(1, 'assistant', 'long answer'), message(2, 'user', 'yes'))
        result = preceding(records, 2, maximum_users=3, maximum_message_bytes=2, maximum_visible=1)
        self.assertEqual(result['stop'], 'visible-record-limit')
        self.assertEqual(result['omitted_records'], 1)
        self.assertIsNone(result['display'][0]['text'])
        for limit in (0, -1, True, 1.5):
            with self.assertRaises(ValueError):
                preceding(records, 2, maximum_visible=limit)

    def test_native_context_excludes_sibling_and_future_records(self):
        records = (message(0, "user", "first", -1), message(1, "assistant", "causal", 0),
                   message(2, "assistant", "unrelated sibling", 0), message(3, "user", "yes", 1),
                   message(4, "assistant", "future", 3))
        result = preceding(records, 3)
        self.assertEqual([m["ordinal"] for m in result["display"]], [0, 1])
        self.assertEqual(result["stop"], "preceding-visible-user")
        self.assertFalse(result["native_log_verified"])

    def test_missing_invalid_and_mixed_parents_never_fall_back(self):
        for records in ((message(0, "user", "first", -1), message(3, "user", "yes", 2)),
                        (message(0, "user", "first", -1), message(1, "user", "yes", 1)),
                        (message(0, "user", "first"), message(1, "user", "yes", 0)),
                        (message(0, "user", "first", -1), message(1, "user", "yes"))):
            with self.assertRaises(ValueError):
                preceding(records, records[-1].ordinal)

    def test_tool_text_never_displayed_but_edits_change_context_pin(self):
        records = (message(0, "user", "first"), message(1, "tool", "private output"),
                   message(2, "assistant", "answer"), message(3, "user", "yes"))
        result = preceding(records, 3)
        self.assertNotIn("private output", str(result))
        self.assertEqual(len(result["traversed"]), 3)
        edited = (*records[:1], message(1, "tool", "edited"), *records[2:])
        self.assertNotEqual(result["context_sha256"], preceding(edited, 3)["context_sha256"])

    def test_limits_and_redactions_are_explicit(self):
        records = (message(0, "user", "first"), message(1, "assistant", "a" * 500),
                   message(2, "user", "yes"))
        limited = preceding(records, 2, maximum_records=1)
        self.assertFalse(limited["bounded_interval_complete"])
        omitted = preceding(records, 2, maximum_message_bytes=40)
        self.assertEqual(omitted["omitted_records"], 1)
        self.assertIsNone(omitted["display"][-1]["text"])
        redacted = preceding(records, 2)
        self.assertEqual(redacted["display"][-1]["display_status"], "redacted")

    def test_duplicate_ordinals_and_nonuser_anchors_rejected(self):
        with self.assertRaises(ValueError):
            preceding((message(0, "user", "a"), Message("other", 0, "user", None, "b", "b")), 0)
        with self.assertRaises(ValueError):
            preceding((message(0, "assistant", "answer"),), 0)

    def test_display_budget_omits_without_truncating_or_claiming_semantic_resolution(self):
        records = (message(0, "user", "earlier user"), message(1, "assistant", "a useful answer"),
                   message(2, "user", "yes"))
        result = preceding(records, 2, maximum_display_bytes=15)
        self.assertEqual(result["omitted_records"], 1)
        self.assertLessEqual(result["display_bytes"], 15)
        self.assertTrue(result["bounded_interval_complete"])
        self.assertIsNone(result["display"][0]["text"])
        self.assertEqual(result["display"][1]["text"], "a useful answer")

    def test_chained_context_keeps_total_record_budget_and_excludes_siblings(self):
        records = (message(0, "user", "task", -1), message(1, "assistant", "proposal", 0),
                   message(2, "user", "yes", 1), message(3, "assistant", "answer", 2),
                   message(4, "user", "go", 3), message(5, "assistant", "sibling", 2),
                   message(6, "user", "again", 4))
        full = preceding(records, 6, maximum_users=3)
        self.assertEqual([m["ordinal"] for m in full["display"]], [0, 1, 2, 3, 4])
        self.assertEqual(full["stop"], "preceding-visible-user")
        limited = preceding(records, 6, maximum_users=3, maximum_records=3)
        self.assertEqual(len(limited["traversed"]), 3)
        self.assertEqual(limited["stop"], "record-limit")

    def test_one_user_default_context_pin_is_preserved(self):
        records = (message(0, "user", "task"), message(1, "user", "yes"))
        self.assertEqual(preceding(records, 1), preceding(records, 1, maximum_users=1))
