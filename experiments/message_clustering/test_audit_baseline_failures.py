"""Generated role headings aren't missing source content; earlier answers can be."""

import unittest

from audit_baseline_failures import evidence_covered
from corpus import Role
from followup_inputs import AnswerScope, Variant, prepare
from test_turns import thread, tokenizer
from turns import extract


class SourceEvidenceTests(unittest.TestCase):
    def test_role_headings_and_multiple_messages_map_to_original_ranges(self):
        source = thread(
            (Role.USER, Role.ASSISTANT), ("Configure the index", "Enable HNSW")
        )
        turn = extract(source)[0]
        inputs = prepare(
            source, (turn,), 0, Variant.CONTEXT_MEAN, tokenizer(), AnswerScope.LAST
        )
        quote = "USER:\nConfigure the index\n\nASSISTANT:\nEnable HNSW"
        self.assertTrue(evidence_covered(source, turn, quote, inputs))
        self.assertIsNone(evidence_covered(source, turn, "USER:", inputs))
        self.assertIsNone(
            evidence_covered(source, turn, "Nonexistent statement", inputs)
        )

    def test_omitted_intermediate_answer_is_detected_and_not_header_artifact(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.ASSISTANT),
            ("Configure the index", "Enable HNSW", "All done"),
        )
        turn = extract(source)[0]
        inputs = prepare(
            source, (turn,), 0, Variant.CONTEXT_MEAN, tokenizer(), AnswerScope.LAST
        )
        quote = "USER:\nConfigure the index\n\nASSISTANT:\nEnable HNSW"
        self.assertFalse(evidence_covered(source, turn, quote, inputs))
        self.assertTrue(evidence_covered(source, turn, "All done", inputs))


if __name__ == "__main__":
    unittest.main()
