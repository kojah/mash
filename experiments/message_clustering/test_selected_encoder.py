"""Pooling boundaries and cache identity for context-bearing inputs."""

import tempfile
import unittest
from pathlib import Path

import numpy as np

from local_encoder import Selection, pool_vectors, selection_mask
from test_turns import FakeEncoder, tokenizer
from turn_artifact import SelectedText, Store


class SelectedEncoder(FakeEncoder):
    def encode_selected(self, texts, selections):
        self.calls += len(texts)
        return np.array(
            [
                [float(selection.start), float(selection.end)]
                if selection
                else [1.0, 0.0]
                for selection in selections
            ]
        )


class SelectedContracts(unittest.TestCase):
    def test_pooling_masks_current_unicode_tokens_without_changing_attention(self):
        text = "previous café current 日本語"
        encoded = (tokenizer().encode(text),)
        attention = np.ones((1, len(encoded[0].ids) + 2), dtype=np.int64)
        attention[0, -2:] = 0
        original = attention.copy()
        start = text.index("current")
        pooled = selection_mask(
            (text,), encoded, attention, (Selection(start=start, end=len(text)),)
        )
        np.testing.assert_array_equal(attention, original)
        self.assertEqual(pooled[0, 0], 0)
        self.assertEqual(pooled[0, len(encoded[0].ids) - 1], 0)
        self.assertEqual(int(pooled.sum()), 2)
        np.testing.assert_array_equal(
            selection_mask((text,), encoded, attention, (None,)), attention
        )
        with self.assertRaises(ValueError):
            selection_mask((text,), encoded, attention, (Selection(start=1, end=99),))
        whitespace = text.index(" ")
        with self.assertRaises(ValueError):
            selection_mask(
                (text,),
                encoded,
                attention,
                (Selection(start=whitespace, end=whitespace + 1),),
            )

    def test_target_pooling_ignores_context_states_and_rejects_invalid_vectors(self):
        hidden = np.array([[[100.0, 0.0], [0.0, 1.0], [0.0, 1.0]]], dtype=np.float32)
        targeted = pool_vectors(hidden, np.array([[0, 1, 1]], dtype=np.int64))
        np.testing.assert_array_equal(targeted, [[0.0, 1.0]])
        self.assertGreater(
            pool_vectors(hidden, np.ones((1, 3), dtype=np.int64))[0, 0], 0.9
        )
        with self.assertRaises(ValueError):
            pool_vectors(hidden, np.zeros((1, 3), dtype=np.int64))
        with self.assertRaises(ValueError):
            pool_vectors(np.zeros_like(hidden), np.ones((1, 3), dtype=np.int64))

    def test_same_text_distinct_selections_and_restart_cache_reuse(self):
        inputs = (
            SelectedText(text="same text"),
            SelectedText(text="same text", selection=Selection(start=0, end=4)),
            SelectedText(text="same text", selection=Selection(start=5, end=9)),
        )
        encoder = SelectedEncoder()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "vectors.sqlite"
            store = Store(path)
            raw, count = store.vectors(("same text",), encoder, lambda *_: None)
            self.assertEqual(count, 1)
            vectors, count = store.selected_vectors(
                inputs + inputs[1:], encoder, lambda *_: None
            )
            self.assertEqual(count, 2)
            np.testing.assert_array_equal(vectors[0], raw[0])
            self.assertFalse(np.array_equal(vectors[1], vectors[2]))
            np.testing.assert_array_equal(vectors[1:3], vectors[3:])
            store.close()
            store = Store(path)
            cached, count = store.selected_vectors(inputs, encoder, lambda *_: None)
            self.assertEqual(count, 0)
            self.assertEqual(encoder.calls, 3)
            np.testing.assert_array_equal(cached, vectors[:3])
            store.close()


if __name__ == "__main__":
    unittest.main()
