"""Offline inference on installed assets; no model acquisition or API calls."""

import hashlib
from pathlib import Path
from typing import Literal

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

from clustering import VectorMatrix, normalized_vectors
from corpus import Record


class EncoderManifest(Record):
    model_sha256: str
    tokenizer_sha256: str
    runtime: str
    tokenizer_runtime: str
    batch: int = 8
    threads: int = 1
    max_tokens: int = 512
    pooling: Literal["masked_mean"] = "masked_mean"
    codec: Literal["symmetric_i8_away_from_zero"] = "symmetric_i8_away_from_zero"


class Encoder:
    """Own one CPU session and a tokenizer with explicit experimental limits."""

    def __init__(self, snapshot: Path) -> None:
        import tokenizers

        model = snapshot / "onnx/model_quantized.onnx"
        tokenizer = snapshot / "tokenizer.json"
        self.manifest = EncoderManifest(
            model_sha256=hashlib.sha256(model.read_bytes()).hexdigest(),
            tokenizer_sha256=hashlib.sha256(tokenizer.read_bytes()).hexdigest(),
            runtime=ort.__version__,
            tokenizer_runtime=tokenizers.__version__,
        )
        self.tokenizer = Tokenizer.from_file(str(tokenizer))
        self.tokenizer.no_truncation()
        self.tokenizer.no_padding()
        options = ort.SessionOptions()
        options.intra_op_num_threads = 1
        options.inter_op_num_threads = 1
        self.session = ort.InferenceSession(
            str(model), options, providers=["CPUExecutionProvider"]
        )

    def encode(self, texts: tuple[str, ...]) -> VectorMatrix:
        """Inputs must fit; padding is batch-local, with no implicit truncation."""
        if not texts:
            return np.empty((0, 384), dtype=np.float64)
        encoded = self.tokenizer.encode_batch(list(texts))
        width = max(len(item.ids) for item in encoded)
        if width > 512:
            raise ValueError("input exceeds explicit encoder budget")
        ids = np.zeros((len(texts), width), dtype=np.int64)
        mask = np.zeros_like(ids)
        types = np.zeros_like(ids)
        for index, item in enumerate(encoded):
            ids[index, : len(item.ids)] = item.ids
            mask[index, : len(item.ids)] = 1
            types[index, : len(item.ids)] = item.type_ids
        hidden = np.asarray(
            self.session.run(
                ["last_hidden_state"],
                {"input_ids": ids, "attention_mask": mask, "token_type_ids": types},
            )[0],
            dtype=np.float32,
        )
        expanded = mask[:, :, None].astype(np.float32)
        mean = (hidden * expanded).sum(axis=1) / expanded.sum(axis=1)
        # Match Rust f32 arithmetic and ties-away-from-zero, not np.rint ties-even.
        mean /= np.linalg.norm(mean, axis=1, keepdims=True)
        scaled = mean * (127 / np.max(np.abs(mean), axis=1, keepdims=True))
        quantized = np.sign(scaled) * np.floor(np.abs(scaled) + 0.5)
        return normalized_vectors(quantized.astype(np.int8).astype(np.float64))
