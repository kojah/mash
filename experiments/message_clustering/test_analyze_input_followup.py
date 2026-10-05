"""Coverage deduplication and complete external resource evidence."""

import tempfile
import unittest
from pathlib import Path

from analyze_input_followup import coverage, peak_rss, union_length
from corpus import Role
from test_turns import thread
from turns import Span, extract


def span(source: int, start: int, end: int) -> Span:
    return Span(source=source, message_id=str(source), start=start, end=end)


class FollowupAnalysis(unittest.TestCase):
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
