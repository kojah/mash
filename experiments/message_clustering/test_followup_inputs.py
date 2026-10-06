"""Original source offsets, ancestry boundaries and explicit token budgets."""

import unittest

from corpus import Role
from followup_inputs import (
    CLOSE_REPLY,
    OPEN_REPLY,
    AnswerScope,
    Variant,
    ancestor,
    lean_prompt,
    prepare,
)
from test_turns import thread, tokenizer
from turns import extract


class FollowupInputs(unittest.TestCase):
    def test_transport_selection_preserves_scalar_text_negation_and_offsets(self):
        payload = (
            OPEN_REPLY + '\n[{"questionItemId":"opaque-nonce",'
            '"question":"Do NOT remove café identifiers?",'
            '"answer":"Keep model_A; do not rename it."}]\n' + CLOSE_REPLY
        )
        source = thread((Role.USER,), (payload,))
        selected = lean_prompt(source, 0)
        self.assertNotIn("opaque-nonce", selected.text)
        self.assertNotIn("questionItemId", selected.text)
        self.assertIn("Do NOT remove café identifiers?", selected.text)
        self.assertIn("Keep model_A; do not rename it.", selected.text)
        for item in selected.locations:
            self.assertEqual(
                selected.text[item.start : item.end],
                payload[item.source.start : item.source.end],
            )

    def test_unknown_malformed_and_empty_envelopes_preserve_original(self):
        for payload in (
            OPEN_REPLY + "invalid" + CLOSE_REPLY,
            OPEN_REPLY + "[]" + CLOSE_REPLY,
            OPEN_REPLY + '[{"answer":"A","extra":"unknown"}]' + CLOSE_REPLY,
            OPEN_REPLY + '[{"questionItemId":"id","answer":""}]' + CLOSE_REPLY,
        ):
            source = thread((Role.USER,), (payload,))
            self.assertEqual(lean_prompt(source, 0).text, payload)

    def test_parent_is_ordinal_and_sibling_segments_are_not_borrowed(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.ASSISTANT, Role.USER),
            ("question", "proposal A", "sibling proposal B", "do A"),
        )
        messages = tuple(
            message.model_copy(
                update={
                    "ordinal": (i + 1) * 10,
                    "context_parent": 20 if i == 3 else None,
                }
            )
            for i, message in enumerate(source.messages)
        )
        source = source.model_copy(update={"messages": messages})
        turns = extract(source)
        previous = ancestor(source, turns, 1)
        self.assertIsNotNone(previous)
        if previous:
            self.assertEqual(previous.sources, (0, 1))
        prepared = prepare(source, turns, 1, Variant.CONTEXT_TARGET, tokenizer())
        self.assertNotIn("sibling proposal B", prepared[0].content.text)
        self.assertIn("proposal A", prepared[0].content.text)

    def test_explicit_roots_and_unknown_parent_abstain(self):
        source = thread((Role.USER, Role.ASSISTANT, Role.USER))
        for parent in (-1, 1):
            messages = tuple(
                message.model_copy(
                    update={
                        "ordinal": i * 10,
                        "context_parent": parent if i == 2 else None,
                    }
                )
                for i, message in enumerate(source.messages)
            )
            changed = source.model_copy(update={"messages": messages})
            turns = extract(changed)
            self.assertIsNone(ancestor(changed, turns, 1))

    def test_context_windows_preserve_current_tokens_and_all_answer_segments(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.USER, Role.ASSISTANT, Role.ASSISTANT),
            (
                "old topic " * 200,
                "old proposal " * 200,
                "current café " * 1000,
                "intermediate update",
                "final selected segment",
            ),
        )
        turns = extract(source)
        prepared = prepare(source, turns, 1, Variant.CONTEXT_TARGET, tokenizer())
        self.assertTrue(all(item.tokens <= 512 for item in prepared))
        self.assertGreater(len([p for p in prepared if p.role == Role.USER]), 1)
        covered = set()
        for item in prepared:
            for located in item.content.locations:
                span = located.source
                self.assertEqual(
                    item.content.text[located.start : located.end],
                    source.messages[span.source].projection[span.start : span.end],
                )
                if span.source == 2:
                    covered.update(range(span.start, span.end))
            if item.role == Role.USER:
                self.assertIsNotNone(item.target)
                if item.target:
                    self.assertEqual(item.target.end, len(item.content.text))
                    self.assertNotIn(
                        "Previous",
                        item.content.text[item.target.start : item.target.end],
                    )
                    for located in item.content.locations:
                        if located.source.source == 2:
                            self.assertGreaterEqual(located.start, item.target.start)
                        else:
                            self.assertLessEqual(located.end, item.target.start)
            else:
                self.assertIn("intermediate update", item.content.text)
                self.assertIn("final selected segment", item.content.text)
        self.assertTrue(
            all(
                i in covered
                for i, c in enumerate(source.messages[2].projection)
                if not c.isspace()
            )
        )

        legacy = prepare(
            source, turns, 1, Variant.CONTEXT_TARGET, tokenizer(), AnswerScope.LAST
        )
        self.assertEqual(
            [i.content.text for i in legacy if i.role == Role.ASSISTANT],
            ["final selected segment"],
        )


if __name__ == "__main__":
    unittest.main()
