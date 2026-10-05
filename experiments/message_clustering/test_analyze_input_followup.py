"""Coverage deduplication and complete external resource evidence."""

import tempfile
import unittest
from pathlib import Path

from analyze_input_followup import coverage, peak_rss, union_length, validate_sources
from build_input_followup import Built
from corpus import Role
from followup_inputs import Variant, prepare
from test_turns import thread, tokenizer
from turn_study_corpus import Phase
from turns import Span, extract


def span(source: int, start: int, end: int) -> Span:
    return Span(source=source, message_id=str(source), start=start, end=end)


class FollowupAnalysis(unittest.TestCase):
    def test_provenance_checks_literal_ranges_identity_and_ancestry(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.USER, Role.ASSISTANT),
            ("proposal", "use this design", "yes", "done"),
        )
        turns = extract(source)
        inputs = prepare(source, turns, 1, Variant.CONTEXT_TARGET, tokenizer())
        built = Built(
            thread_index=0,
            phase=Phase.DEVELOPMENT,
            variant=Variant.CONTEXT_TARGET,
            seconds=0,
            new_vectors=0,
            input_tokens=0,
            matrix_sha256="unused",
            turn_ids=tuple(turn.id for turn in turns),
            inputs=inputs,
        )
        validate_sources(built, source, turns)
        item = inputs[0]
        bad_location = item.content.locations[0].model_copy(
            update={
                "source": item.content.locations[0].source.model_copy(
                    update={"message_id": "wrong source"}
                )
            }
        )
        bad_content = item.content.model_copy(
            update={"locations": (bad_location,) + item.content.locations[1:]}
        )
        for changed in (
            item.model_copy(update={"context_turn_id": "unrelated"}),
            item.model_copy(update={"content": bad_content}),
            item.model_copy(
                update={
                    "content": item.content.model_copy(
                        update={"text": "changed" + item.content.text}
                    )
                }
            ),
        ):
            with self.assertRaises(ValueError):
                validate_sources(
                    built.model_copy(update={"inputs": (changed,)}), source, turns
                )
        future = inputs[-1].model_copy(update={"turn_id": turns[0].id})
        with self.assertRaises(ValueError):
            validate_sources(
                built.model_copy(update={"inputs": (future,)}), source, turns
            )

    def test_overlapping_windows_count_once_and_context_is_not_own_coverage(self):
        turns = extract(
            thread(
                (Role.USER, Role.ASSISTANT, Role.USER, Role.ASSISTANT),
                ("abcdef", "abcde", "xyz", "answer"),
            )
        )
        spans = {
            turns[0].id: (span(0, 0, 4), span(0, 2, 6)),
            turns[1].id: (span(2, 0, 2), span(3, 0, 6), span(0, 0, 4), span(0, 2, 6)),
        }
        result = coverage(turns, (6, 5, 3, 6), spans, frozenset((turns[1].id,)))
        self.assertEqual(result.own_source_characters, 20)
        self.assertEqual(result.supplied_own_characters, 14)
        self.assertEqual(result.supplied_context_characters, 6)
        self.assertEqual(result.entirely_excluded_sources, 1)
        self.assertEqual(result.partially_supplied_sources, 1)
        self.assertEqual(result.context_turns, 1)
        with self.assertRaises(ValueError):
            coverage(
                turns[:1], (6, 5, 3, 6), {turns[0].id: (span(0, 0, 7),)}, frozenset()
            )

    def test_union_handles_reordered_contained_disjoint_and_invalid_ranges(self):
        self.assertEqual(union_length(((8, 10), (1, 5), (2, 3), (4, 7))), 8)
        self.assertEqual(union_length(()), 0)
        for interval in ((-1, 2), (3, 2)):
            with self.assertRaises(ValueError):
                union_length((interval,))

    def test_resource_gate_requires_terminal_success_and_measured_peak(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "time.txt"
            path.write_text(
                "\tMaximum resident set size (kbytes): 1234\n\tExit status: 0\n"
            )
            self.assertEqual(peak_rss(path), 1234)
            for text in (
                "Maximum resident set size (kbytes): 1234\n",
                "Maximum resident set size (kbytes): 1234\nExit status: 1\n",
                "Exit status: 0\n",
            ):
                path.write_text(text)
                with self.assertRaises(ValueError):
                    peak_rss(path)


if __name__ == "__main__":
    unittest.main()
