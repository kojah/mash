import unittest

from native_projection_audit import category, flatten, stable_message


class NativeAuditTests(unittest.TestCase):
    def test_provider_user_tool_results_are_context_not_human_prose(self):
        value = [{'type': 'tool_result', 'content': [{'type': 'text', 'text': 'tool output'}]}]
        self.assertEqual(flatten(value), 'tool output')
        self.assertEqual(category(value), 'native-tool-result-only')

    def test_mixed_content_retains_original_text_and_unknowns_are_not_guessed(self):
        value = [{'type': 'tool_result', 'content': 'output'}, {'type': 'text', 'text': 'request'}]
        self.assertEqual(flatten(value), 'output\nrequest')
        self.assertEqual(category(value), 'native-mixed-or-unknown-blocks')
        self.assertIsNone(flatten({'type': 'unknown-container', 'content': [{'text': 'hidden'}]}))

    def test_native_identity_includes_thread_and_uuid(self):
        self.assertNotEqual(stable_message('one', 'x'), stable_message('two', 'x'))
        self.assertNotEqual(stable_message('one', 'x'), stable_message('one', 'y'))
        self.assertEqual(len(stable_message('one', 'x')), 24)


if __name__ == '__main__':
    unittest.main()
