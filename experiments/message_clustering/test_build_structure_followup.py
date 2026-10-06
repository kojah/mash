"""Segmentation cannot silently drop prompts or restore excluded answers."""

import unittest

from build_structure_followup import check_coverage
from corpus import Role
from followup_inputs import AnswerScope, Input, Variant, prepare, source_text
from test_turns import thread, tokenizer
from turns import extract


class CoverageContracts(unittest.TestCase):
    def test_non_token_citation_marker_is_preserved_even_if_old_window_omitted_it(self):
        source = thread(
            (Role.USER, Role.ASSISTANT), ("Question", "\ue200citation text")
        )
        turns = extract(source)
        current = prepare(source, turns, 0, Variant.CONTEXT_MEAN, tokenizer())
        full = source_text(source, 1)
        kept = current[-1].model_copy(update={"content": full})
        old = current[:-1] + (
            kept.model_copy(update={"content": full.slice(1, len(full.text))}),
        )
        check_coverage(old, current[:-1] + (kept,), source, turns)
        with self.assertRaises(ValueError):
            check_coverage(old, old, source, turns)

    def test_missing_prompt_text_fails_selected_source_coverage(self):
        source = thread((Role.USER, Role.ASSISTANT), ("Do NOT remove A.", "Keep it."))
        turns = extract(source)
        original = prepare(source, turns, 0, Variant.CONTEXT_MEAN, tokenizer())
        check_coverage(original, original, source, turns)
        with self.assertRaises(ValueError):
            check_coverage(original, original[1:], source, turns)

    def test_restoring_intermediate_answer_is_a_different_treatment(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.ASSISTANT),
            ("Question", "Intermediate answer", "Final answer"),
        )
        turns = extract(source)
        original = prepare(
            source, turns, 0, Variant.CONTEXT_MEAN, tokenizer(), AnswerScope.LAST
        )
        added = Input(
            turn_id=turns[0].id,
            role=Role.ASSISTANT,
            content=source_text(source, 1),
            tokens=2,
        )
        with self.assertRaises(ValueError):
            check_coverage(
                original, original + (added,), source, turns, AnswerScope.LAST
            )

    def test_default_coverage_requires_earlier_explanation_code_and_final_status(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.ASSISTANT, Role.ASSISTANT),
            (
                "Explain the index",
                "Detailed explanation",
                "```rust\nlet index = build();\n```",
                "Done",
            ),
        )
        turns = extract(source)
        complete = prepare(source, turns, 0, Variant.CONTEXT_MEAN, tokenizer())
        check_coverage(complete, complete, source, turns)
        legacy = prepare(
            source, turns, 0, Variant.CONTEXT_MEAN, tokenizer(), AnswerScope.LAST
        )
        with self.assertRaises(ValueError):
            check_coverage(legacy, legacy, source, turns)


if __name__ == "__main__":
    unittest.main()
