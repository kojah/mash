"""Protect payload, source boundaries and exact unweighted pooling behavior."""

import unittest

import numpy as np
from tokenizers import Tokenizer
from tokenizers.models import WordLevel
from tokenizers.pre_tokenizers import Whitespace

from followup_inputs import Text
from local_encoder import Selection, pool_vectors, weight_mask
from local_representation import best_context, candidates, filler_spans


class RepresentationTest(unittest.TestCase):
    def test_context_selection_and_abstention(self) -> None:
        spans = (Text(text="irrelevant"), Text(text="database"))
        vectors = np.eye(2)
        self.assertEqual(best_context(spans, vectors, np.array([0.0, 1.0])), spans[1])
        self.assertEqual(best_context(spans, vectors, np.array([1.0, 1.0])), spans[0])
        self.assertFalse(best_context(spans, vectors, np.array([-1.0, -1.0])).text)

    def test_filler_protects_payload_and_code(self) -> None:
        text = (
            "Happy to help! Use FTS5 for lexical search. "
            "Let me know if you have any questions."
        )
        spans = filler_spans(text)
        self.assertEqual(
            [text[s.start : s.end] for s in spans],
            ["Happy to help!", "Let me know if you have any questions."],
        )
        self.assertFalse(filler_spans("Happy to help debug the database timeout."))
        self.assertFalse(filler_spans("```text\nHappy to help!\n```"))
        self.assertFalse(filler_spans("`Happy to help!`"))
        self.assertFalse(
            filler_spans("Repeated database timeout. Repeated database timeout.")
        )

    def test_weights_and_no_span_parity(self) -> None:
        tokenizer = Tokenizer(
            WordLevel({"[UNK]": 0, "hello": 1, "database": 2}, unk_token="[UNK]")
        )
        tokenizer.pre_tokenizer = Whitespace()
        text = "hello database"
        encoded = (tokenizer.encode(text),)
        mask = np.ones((1, 2), dtype=np.int64)
        hidden = np.array([[[1, 0], [0, 1]]], dtype=np.float32)
        same = weight_mask((text,), encoded, mask, ((),))
        np.testing.assert_array_equal(
            pool_vectors(hidden, same), pool_vectors(hidden, mask)
        )
        weighted = weight_mask((text,), encoded, mask, ((Selection(start=0, end=5),),))
        np.testing.assert_array_equal(weighted, [[1, 4]])
        vector = pool_vectors(hidden, weighted)[0]
        self.assertGreater(vector[1], 3.9 * vector[0])
        with self.assertRaisesRegex(ValueError, "outside"):
            weight_mask((text,), encoded, mask, ((Selection(start=0, end=99),),))

    def test_candidate_sentences_preserve_technical_punctuation(self) -> None:
        tokenizer = Tokenizer(WordLevel({"[UNK]": 0}, unk_token="[UNK]"))
        tokenizer.pre_tokenizer = Whitespace()
        result = candidates(
            Text(text="Use pgvector.v1 for db. Fix API timeouts!"), tokenizer
        )
        self.assertEqual(
            [r.text.strip() for r in result],
            ["Use pgvector.v1 for db.", "Fix API timeouts!"],
        )


if __name__ == "__main__":
    unittest.main()
