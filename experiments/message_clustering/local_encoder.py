"""Offline inference on installed assets; no model acquisition or API calls."""

import hashlib
from pathlib import Path
from typing import Literal

import numpy as np
import onnxruntime as ort
import orjson
from numpy.typing import NDArray
from pydantic import ConfigDict, Field
from tokenizers import Encoding, Tokenizer

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


class ModelMetadata(Record):
    model_config = ConfigDict(extra="ignore", frozen=True)
    hidden_size: int = Field(gt=0)
    max_position_embeddings: int = Field(gt=0)
    pad_token_id: int = Field(default=0, ge=0)


class TokenizerMetadata(Record):
    model_config = ConfigDict(extra="ignore", frozen=True)
    model_max_length: int | None = None


def local_capacity(snapshot: Path) -> tuple[int, int, int]:
    model = ModelMetadata.model_validate(
        orjson.loads((snapshot / "config.json").read_bytes())
    )
    token_path = snapshot / "tokenizer_config.json"
    tokenizer = (
        TokenizerMetadata.model_validate_json(token_path.read_bytes())
        if token_path.exists()
        else TokenizerMetadata()
    )
    limit = min(
        model.max_position_embeddings,
        tokenizer.model_max_length or model.max_position_embeddings,
    )
    if limit <= 0:
        raise ValueError("local model has an invalid context limit")
    return model.hidden_size, limit, model.pad_token_id


class Selection(Record):
    start: int = Field(ge=0)
    end: int = Field(gt=0)


def selection_mask(
    texts: tuple[str, ...],
    encoded: tuple[Encoding, ...],
    mask: NDArray[np.int64],
    selections: tuple[Selection | None, ...],
) -> NDArray[np.int64]:
    """Pool selected overlapping token states; attention still sees the full input."""
    if len(texts) != len(encoded) or len(texts) != len(selections):
        raise ValueError("each input needs an encoding and pooling selection")
    if mask.ndim != 2 or len(mask) != len(texts):
        raise ValueError("attention mask shape does not match inputs")
    selected = mask.copy()
    for index, region in enumerate(selections):
        if region is None:
            continue
        if not 0 <= region.start < region.end <= len(texts[index]):
            raise ValueError("pooling selection is outside input characters")
        selected[index] = 0
        for token, (left, right) in enumerate(encoded[index].offsets):
            if right > left and left < region.end and right > region.start:
                selected[index, token] = mask[index, token]
    if np.any(selected.sum(axis=1) == 0):
        raise ValueError("pooling selection contains no nonpadding token states")
    return selected


def pool_vectors(
    hidden: NDArray[np.float32], mask: NDArray[np.int64] | NDArray[np.float32]
) -> VectorMatrix:
    """The original f32 mean/L2/i8 codec, shared by whole and targeted pooling."""
    if hidden.ndim != 3 or hidden.shape[:2] != mask.shape:
        raise ValueError("hidden states and pooling mask shapes differ")
    expanded = mask[:, :, None].astype(np.float32)
    if np.any(expanded.sum(axis=1) == 0):
        raise ValueError("pooling mask is empty")
    mean = (hidden * expanded).sum(axis=1) / expanded.sum(axis=1)
    norms = np.linalg.norm(mean, axis=1, keepdims=True)
    if np.any(norms == 0) or not np.isfinite(norms).all():
        raise ValueError("pooled vectors have invalid norms")
    mean /= norms
    scaled = mean * (127 / np.max(np.abs(mean), axis=1, keepdims=True))
    quantized = np.sign(scaled) * np.floor(np.abs(scaled) + 0.5)
    return normalized_vectors(quantized.astype(np.int8).astype(np.float64))


class Encoder:
    """Own one CPU session and a tokenizer with explicit experimental limits."""

    def __init__(self, snapshot: Path) -> None:
        import tokenizers

        self.dimensions, context, self.pad_token_id = local_capacity(snapshot)
        model = snapshot / "onnx/model_quantized.onnx"
        if not model.exists():
            model = snapshot / "onnx/model.onnx"
        tokenizer = snapshot / "tokenizer.json"
        self.manifest = EncoderManifest(
            model_sha256=hashlib.sha256(model.read_bytes()).hexdigest(),
            tokenizer_sha256=hashlib.sha256(tokenizer.read_bytes()).hexdigest(),
            runtime=ort.__version__,
            tokenizer_runtime=tokenizers.__version__,
            max_tokens=context,
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
        return self.encode_selected(texts, (None,) * len(texts))

    def encode_selected(
        self,
        texts: tuple[str, ...],
        selections: tuple[Selection | None, ...],
        downweighted: tuple[tuple[Selection, ...], ...] | None = None,
    ) -> VectorMatrix:
        """Selections change output pooling, not transformer attention inputs.

        Callers caching this method must include the selections in their keys;
        the original EncoderManifest/cache generation describes whole-input pooling.
        """
        if len(texts) != len(selections):
            raise ValueError("each input needs a pooling selection")
        if not texts:
            return np.empty((0, self.dimensions), dtype=np.float64)
        encoded, mask, hidden = self.states(texts)
        pooling = selection_mask(texts, encoded, mask, selections)
        if downweighted is not None:
            pooling = weight_mask(texts, encoded, pooling, downweighted)
        return pool_vectors(hidden, pooling)

    def states(
        self, texts: tuple[str, ...]
    ) -> tuple[tuple[Encoding, ...], NDArray[np.int64], NDArray[np.float32]]:
        """Expose one forward pass for label-free pooling ablations only."""
        if not texts:
            raise ValueError("hidden-state inference needs nonempty inputs")
        encoded = tuple(self.tokenizer.encode_batch(list(texts)))
        width = max(len(item.ids) for item in encoded)
        if width > self.manifest.max_tokens:
            raise ValueError("input exceeds explicit encoder budget")
        ids = np.full((len(texts), width), self.pad_token_id, dtype=np.int64)
        mask = np.zeros_like(ids)
        types = np.zeros_like(ids)
        for index, item in enumerate(encoded):
            ids[index, : len(item.ids)] = item.ids
            mask[index, : len(item.ids)] = 1
            types[index, : len(item.ids)] = item.type_ids
        available = {item.name for item in self.session.get_inputs()}
        inputs = {"input_ids": ids, "attention_mask": mask, "token_type_ids": types}
        hidden = np.asarray(
            self.session.run(
                ["last_hidden_state"],
                {name: value for name, value in inputs.items() if name in available},
            )[0],
            dtype=np.float32,
        )
        if hidden.shape != (len(texts), width, self.dimensions):
            raise ValueError("model hidden-state shape differs from its metadata")
        return encoded, mask, hidden


def weight_mask(
    texts: tuple[str, ...],
    encoded: tuple[Encoding, ...],
    mask: NDArray[np.int64],
    spans: tuple[tuple[Selection, ...], ...],
) -> NDArray[np.int64]:
    """Relative 4/1 pooling weights; no spans preserves original mask exactly."""
    if len(spans) != len(texts) or len(encoded) != len(texts):
        raise ValueError("each input needs its own weighting spans")
    result = mask.copy()
    for row, regions in enumerate(spans):
        if not regions:
            continue
        if any(not 0 <= r.start < r.end <= len(texts[row]) for r in regions):
            raise ValueError("weighting region outside input")
        result[row] *= 4
        for token, (left, right) in enumerate(encoded[row].offsets):
            if right > left and any(left < r.end and right > r.start for r in regions):
                result[row, token] = mask[row, token]
    return result
