from types import SimpleNamespace
import unittest

import numpy as np

from context_embeddings import Representation, embed, inputs


def entry(ordinal, role, text, status="included"):
    return {"ordinal": ordinal, "role": role, "text": text, "display_status": status}


class ContextEmbeddingTests(unittest.TestCase):
    def test_contextual_input_uses_source_and_preserves_anchor(self):
        context = {"stop": "root", "display": [entry(0, "user", "task"), entry(1, "assistant", "proposal")]}
        self.assertEqual(inputs("yes", context, True, Representation.JOINED), ("task\n\nproposal\n\nyes",))
        self.assertEqual(inputs("yes", context, True, Representation.MEAN), ("task", "proposal", "yes"))
        self.assertEqual(inputs("specific task", context, False, Representation.JOINED), ("specific task",))

    def test_incomplete_or_over_cap_context_abstains_instead_of_truncating(self):
        for context in (
            {"stop": "record-limit", "display": [entry(1, "assistant", "partial")]},
            {"stop": "root", "display": [entry(0, "user", None, "omitted-size"), entry(1, "assistant", "proposal")]},
            {"stop": "root", "display": [entry(0, "user", "task")] + [entry(i, "assistant", "answer") for i in range(1, 5)]},
        ):
            self.assertEqual(inputs("yes", context, True, Representation.JOINED), ())

    def test_overflow_rejects_entire_input_before_inference(self):
        calls = []
        encoder = SimpleNamespace(tokenizer=SimpleNamespace(encode=lambda t: SimpleNamespace(ids=range(int(t)))),
                                  encode=lambda t: calls.append(t))
        vector, lengths = embed(("10", "513"), encoder)
        self.assertIsNone(vector)
        self.assertEqual(lengths, (10, 513))
        self.assertEqual(calls, [])

    def test_record_mean_cancellation_abstains(self):
        encoder = SimpleNamespace(tokenizer=SimpleNamespace(encode=lambda t: SimpleNamespace(ids=range(3))),
                                  encode=lambda t: np.array([1., 0.]) if t == "a" else np.array([-1., 0.]))
        self.assertIsNone(embed(("a", "b"), encoder)[0])
