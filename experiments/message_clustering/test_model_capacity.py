"""Model-dependent capacity, cache separation and source-preserving boundaries."""

import tempfile
import unittest
from pathlib import Path

import orjson
from tokenizers import Tokenizer, models, pre_tokenizers

from local_encoder import local_capacity
from model_capacity import Capacity, TokenBudget
from voyage_vectors import Item, Model, Response, Usage, cache_space, decode, digest


class CapacityTests(unittest.TestCase):
    def test_dimensions_and_context_are_model_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "config.json").write_bytes(
                orjson.dumps(
                    {
                        "hidden_size": 768,
                        "max_position_embeddings": 4096,
                        "pad_token_id": 9,
                    }
                )
            )
            (root / "tokenizer_config.json").write_bytes(
                orjson.dumps({"model_max_length": 2048})
            )
            self.assertEqual(local_capacity(root), (768, 2048, 9))

    def test_lossless_token_limit_can_exceed_old_512_limit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tokenizer.json"
            tokenizer = Tokenizer(
                models.WordLevel({"[UNK]": 0, "a": 1, "東京": 2}, unk_token="[UNK]")
            )
            tokenizer.pre_tokenizer = pre_tokenizers.Whitespace()
            tokenizer.save(str(path))
            capacity = Capacity(
                model=Model.GENERAL,
                dimensions=2048,
                context_tokens=2048,
                input_tokens=700,
                batch_tokens=1000,
                tokenizer_sha256=digest(path.read_bytes()),
            )
            budget = TokenBudget(path, capacity)
            text = "a " * 900 + "\n\n東京 " * 20
            parts = budget.parts(text)
            self.assertEqual("".join(parts), text)
            self.assertTrue(all(budget.count(p) <= 700 for p in parts))
            self.assertGreater(max(budget.count(p) for p in parts), 512)
            self.assertEqual(len(budget.batches(("a " * 600, "a " * 600))), 2)
            self.assertNotEqual(
                cache_space(Model.GENERAL, capacity),
                cache_space(
                    Model.GENERAL, capacity.model_copy(update={"dimensions": 1024})
                ),
            )
            self.assertEqual(
                cache_space(Model.GENERAL, capacity),
                cache_space(
                    Model.GENERAL, capacity.model_copy(update={"input_tokens": 1000})
                ),
            )

    def test_response_shape_uses_requested_dimensions(self) -> None:
        response = Response(
            model="voyage-4",
            data=(Item(index=0, embedding=(1.0,) * 2048),),
            usage=Usage(total_tokens=1),
        )
        self.assertEqual(decode(response, 1, Model.GENERAL, 2048).shape, (1, 2048))
        with self.assertRaises(ValueError):
            decode(response, 1, Model.GENERAL, 1024)

    def test_invalid_capabilities_fail_before_inference(self) -> None:
        with self.assertRaises(ValueError):
            Capacity(model=Model.CODE, dimensions=384, tokenizer_sha256="synthetic")
        with self.assertRaises(ValueError):
            Capacity(model=Model.CODE, input_tokens=40000, tokenizer_sha256="synthetic")


if __name__ == "__main__":
    unittest.main()
