"""Structural boundaries, exact coverage and explicit token-limit fallback."""

import unittest

from corpus import Role
from followup_inputs import Text, source_text
from structure_chunks import Kind, Mode, blocks, bounded, prepare_structured
from test_turns import thread, tokenizer
from turns import extract


class StructureChunks(unittest.TestCase):
    def test_short_turn_keeps_one_point_per_role_and_messages_never_merge(self):
        source = thread(
            (Role.USER, Role.USER, Role.ASSISTANT),
            ("First useful request.", "Second useful request.", "Useful response."),
        )
        turns = extract(source)
        prepared = prepare_structured(source, turns, 0, Mode.PLAIN, tokenizer())
        self.assertEqual(len(prepared), 3)
        self.assertEqual(
            [item.input.role for item in prepared],
            [Role.USER, Role.USER, Role.ASSISTANT],
        )
        self.assertEqual(
            [
                tuple(
                    location.source.source for location in item.input.content.locations
                )
                for item in prepared
            ],
            [(0,), (1,), (2,)],
        )

    def test_headings_code_and_unclosed_fences_define_sections(self):
        text = (
            "# Alpha\nParagraph café.\n\n```rust\n    # not a heading\n```\n\n"
            "# Beta\n~~~python\n    unfinished()\n"
        )
        parsed = blocks(text)
        self.assertEqual(
            [block.kind for block in parsed],
            [Kind.HEADING, Kind.PROSE, Kind.CODE, Kind.HEADING, Kind.CODE],
        )
        self.assertEqual(parsed[-1].end, len(text))
        self.assertEqual(parsed[0].section, parsed[2].section)
        self.assertNotEqual(parsed[2].section, parsed[3].section)
        source = thread((Role.USER,), (text,))
        result = prepare_structured(source, extract(source), 0, Mode.PLAIN, tokenizer())
        self.assertEqual(len(result), 3)
        self.assertIn("    # not a heading", result[1].input.content.text)
        self.assertTrue(result[1].input.content.text.endswith("```"))
        self.assertFalse(
            any(
                "Alpha" in item.input.content.text and "Beta" in item.input.content.text
                for item in result
            )
        )

    def test_paragraph_packing_and_heading_copy_do_not_duplicate_first_heading(self):
        paragraph = " ".join(["meaningful"] * 90)
        text = "# Domain\n" + paragraph + "\n\n" + paragraph
        source = thread((Role.USER,), (text,))
        turns = extract(source)
        plain = prepare_structured(source, turns, 0, Mode.PLAIN, tokenizer())
        guided = prepare_structured(source, turns, 0, Mode.HEADING, tokenizer())
        self.assertEqual(len(plain), 2)
        self.assertEqual(len(guided), 2)
        self.assertEqual(guided[0].input.content.text.count("Domain"), 1)
        self.assertTrue(guided[1].input.content.text.startswith("Section:\n# Domain\n"))
        self.assertNotIn("Domain", plain[1].input.content.text)

    def test_oversized_atoms_record_fallback_and_preserve_unicode_source_coverage(self):
        text = (
            "# Résumé\n"
            + " ".join(["café"] * 1200)
            + "\n\n```\n"
            + " ".join(["identifier"] * 1200)
            + "\n```"
        )
        source = thread((Role.USER,), (text,))
        for mode in Mode:
            prepared = prepare_structured(source, extract(source), 0, mode, tokenizer())
            self.assertTrue(any(item.forced_token_split for item in prepared))
            self.assertTrue(all(item.input.tokens <= 512 for item in prepared))
            covered = set()
            for item in prepared:
                for location in item.input.content.locations:
                    span = location.source
                    self.assertEqual(
                        item.input.content.text[location.start : location.end],
                        text[span.start : span.end],
                    )
                    covered.update(range(span.start, span.end))
            self.assertTrue(
                all(
                    index in covered
                    for index, char in enumerate(text)
                    if not char.isspace()
                )
            )

    def test_sentence_and_code_line_splits_avoid_forced_tokens_when_atoms_fit(self):
        source = thread(
            (Role.USER,), ("A sentence. B sentence. C sentence. D sentence.",)
        )
        prose = bounded(source_text(source, 0), Kind.PROSE, tokenizer(), 8)
        self.assertGreater(len(prose), 1)
        self.assertFalse(any(part.forced_token_split for part in prose))
        code = bounded(
            Text(text="one two\nthree four\nfive six\n"), Kind.CODE, tokenizer(), 4
        )
        self.assertEqual(
            [part.content.text for part in code], ["one two\nthree four", "five six"]
        )
        self.assertFalse(any(part.forced_token_split for part in code))


if __name__ == "__main__":
    unittest.main()
