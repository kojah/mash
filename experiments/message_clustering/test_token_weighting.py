"""Protection, weighting coverage and local-statistics contracts."""

import unittest

import numpy as np
from tokenizers import Tokenizer, models, pre_tokenizers

from local_encoder import pool_vectors
from test_turns import tokenizer
from token_weighting import Frequencies, Policy, weights, words


class TokenWeightingTests(unittest.TestCase):
    def test_real_wordpiece_subtokens_inherit_the_same_weight(self):
        encoder = Tokenizer(
            models.WordPiece(
                {"[UNK]": 0, "bas": 1, "##ically": 2, "not": 3}, unk_token="[UNK]"
            )
        )
        encoder.pre_tokenizer = pre_tokenizers.Whitespace()
        text = "basically not"
        encoded = encoder.encode(text)
        self.assertEqual(encoded.tokens, ["bas", "##ically", "not"])
        mask = np.ones(len(encoded.ids), dtype=np.int64)
        result = weights(
            text, encoded, mask, Frequencies.from_messages((text,)), Policy.FUNCTION
        )
        np.testing.assert_array_equal(result, [0.25, 0.25, 1])

    def test_curly_apostrophe_negations_are_whole_protected_words(self):
        text = "don’t isn’t can’t won’t shouldn't hasn’t haven’t needn't the"
        self.assertEqual(tuple(w.text for w in words(text)), ("the",))
        frequency = Frequencies.from_messages((text,))
        self.assertEqual(set(frequency.counts), {"the"})

    def test_code_identifiers_and_negation_are_protected(self):
        text = (
            "the not never Qdrant foo_bar abc123 path/to/the "
            "`the` ```\nthe\n``` database"
        )
        self.assertEqual(tuple(w.text for w in words(text)), ("the", "database"))
        frequency = Frequencies.from_messages((text,))
        encoded = tokenizer().encode(text)
        original = np.ones(len(encoded.ids), dtype=np.int64)
        result = weights(text, encoded, original, frequency, Policy.FUNCTION)
        for index, (left, right) in enumerate(encoded.offsets):
            expected = 0.25 if left < 3 and right > 0 else 1
            self.assertEqual(result[index], expected)
        np.testing.assert_array_equal(original, np.ones_like(original))

    def test_subtokens_share_word_weight_specials_stay_full(self):
        text = "the database"
        encoded = tokenizer().encode(text)
        mask = np.ones(len(encoded.ids), dtype=np.int64)
        frequency = Frequencies.from_messages((text, "the database", "the"))
        for policy in Policy:
            result = weights(text, encoded, mask, frequency, policy)
            self.assertEqual(result[0], 1)
            self.assertEqual(result[-1], 1)
            self.assertTrue(np.all((result > 0) & (result <= 1)))
        self.assertLess(
            frequency.weight("the", Policy.IDF),
            frequency.weight("database", Policy.IDF),
        )
        self.assertLess(frequency.weight("the", Policy.SIF), 1)
        self.assertEqual(frequency.weight("unseen", Policy.IDF), 1)

    def test_uniform_pooling_is_identical_and_weights_change_direction(self):
        hidden = np.array([[[1, 0], [0, 1], [0, 1]]], dtype=np.float32)
        np.testing.assert_array_equal(
            pool_vectors(hidden, np.ones((1, 3), dtype=np.int64)),
            pool_vectors(hidden, np.ones((1, 3), dtype=np.float32)),
        )
        weighted = pool_vectors(hidden, np.array([[0.25, 1, 1]], dtype=np.float32))
        self.assertLess(weighted[0, 0], 0.2)

    def test_frequency_uses_original_messages_not_other_threads(self):
        messages = ("the database", "the database", "the index")
        own = Frequencies.from_messages(messages)
        Frequencies.from_messages(("unrelated the the the",))
        self.assertEqual(own, Frequencies.from_messages(messages))
        self.assertEqual(own.df["database"], 2)
        self.assertEqual(own.counts["the"], 3)
        for policy in Policy:
            self.assertEqual(
                Frequencies.from_messages(()).weight("the", policy),
                0.25 if policy == Policy.FUNCTION else 1,
            )


if __name__ == "__main__":
    unittest.main()
