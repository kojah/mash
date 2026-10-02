import unittest

import numpy as np

from reference_review import digest
from source_windows import incidence, windows


class SourceWindowTests(unittest.TestCase):
    def test_all_tokens_count_before_candidate_filtering(self):
        rows = windows('alpha the and beta', ['alpha', 'beta'], 2)
        self.assertEqual([w.words for w in rows], [(0,), (1,)])
        self.assertEqual([w.token_count for w in rows], [2, 2])
        np.testing.assert_array_equal(incidence(rows, 2), [[1, 0], [0, 1]])

    def test_complete_unicode_byte_and_codepoint_spans(self):
        text = '🙂 α\r\n e\u0301 beta\u00a0gamma!'
        rows = windows(text, ['α', 'e', 'beta', 'gamma'], 1)
        self.assertEqual(rows[0].start, 0)
        self.assertEqual(rows[-1].end, len(text))
        self.assertEqual(''.join(text[w.start:w.end] for w in rows), text)
        for w in rows:
            piece = text[w.start:w.end]
            self.assertEqual(piece.encode(), text.encode()[w.byte_start:w.byte_end])
            self.assertEqual(w.text_sha256, digest(piece.encode()))
        for left, right in zip(rows, rows[1:]):
            self.assertEqual(left.end, right.start)
            self.assertEqual(left.byte_end, right.byte_start)

    def test_repeated_words_one_vote_and_empty_windows_retained(self):
        rows = windows('alpha alpha the the alpha', ['alpha'], 2)
        self.assertEqual([w.words for w in rows], [(0,), (), (0,)])
        self.assertEqual([w.token_count for w in rows], [2, 2, 1])
        for text in ('', '🙂  \r\n'):
            empty = windows(text, ['alpha'], 2)
            self.assertEqual(len(empty), 1)
            self.assertEqual(empty[0].words, ())
            self.assertEqual(empty[0].end, len(text))

    def test_source_boundaries_never_stitch(self):
        records = windows('alpha', ['alpha', 'beta'], 16) + windows('beta', ['alpha', 'beta'], 16)
        self.assertEqual((incidence(records, 2).astype(int).T @ incidence(records, 2))[0, 1], 0)

    def test_invalid_budget_vocabulary_and_caps_fail(self):
        for budget in (0, -1, 1.5, True):
            with self.assertRaises(ValueError):
                windows('a', ['a'], budget)
        for words in (['a', 'a'], ['a b'], ['A'], ['']):
            with self.assertRaises(ValueError):
                windows('a', words, 2)
        with self.assertRaises(ValueError):
            incidence(windows('a', ['a'], 1) * 4097, 1)
        with self.assertRaises(ValueError):
            incidence(windows('a', ['a'], 1), 0)


if __name__ == '__main__':
    unittest.main()
