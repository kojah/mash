"""Literal token boundaries and stable scores, without inference in unit tests."""

import unittest

from tokenizers import Tokenizer, models, pre_tokenizers

from local_affinity import prefix, sigmoid


class AffinityTests(unittest.TestCase):
    def test_literal_prefix(self) -> None:
        tokenizer = Tokenizer(models.WordLevel({"[UNK]": 0, "é": 1}, unk_token="[UNK]"))
        tokenizer.pre_tokenizer = pre_tokenizers.Whitespace()
        text = " ".join(["é"] * 520)
        selected = prefix(text, tokenizer)
        self.assertEqual(selected.text, " ".join(["é"] * 510))
        self.assertEqual(selected.full_tokens, 520)
        self.assertEqual(selected.retained_tokens, 510)
        self.assertTrue(selected.truncated)
        self.assertEqual(prefix("é", tokenizer).text, "é")
        with self.assertRaises(ValueError):
            prefix("", tokenizer)

    def test_sigmoid_extremes(self) -> None:
        self.assertEqual(sigmoid(0), 0.5)
        self.assertEqual(sigmoid(1000), 1)
        self.assertEqual(sigmoid(-1000), 0)
        for logit in (float("inf"), float("nan")):
            with self.assertRaises(ValueError):
                sigmoid(logit)


if __name__ == "__main__":
    unittest.main()
