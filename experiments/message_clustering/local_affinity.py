"""Pinned offline cross-encoder with explicit literal prefix budgets."""

import hashlib
import math
from pathlib import Path

import numpy as np
import onnxruntime as ort
import tokenizers
from tokenizers import Tokenizer

from corpus import Record

MODEL_SHA = "c1296c66c119de645fa9cdee536d8637740efe85224cfa270281e50f213aa565"
TOKENIZER_SHA = "0046da43cc8c424b317f56b092b0512aaaa65c4f925d2f16af9d9eeb4d0ef902"
SIDE_TOKENS = 510
PAIR_TOKENS = 1024


class Manifest(Record):
    model_sha256: str = MODEL_SHA
    tokenizer_sha256: str = TOKENIZER_SHA
    runtime: str
    tokenizer_runtime: str
    side_tokens: int = SIDE_TOKENS
    pair_tokens: int = PAIR_TOKENS
    threads: int = 1
    batch: int = 1


class Prefix(Record):
    text: str
    full_tokens: int
    retained_tokens: int
    truncated: bool


def prefix(text: str, tokenizer: Tokenizer) -> Prefix:
    """Retain source characters; never decode tokens into reconstructed prose."""
    encoded = tokenizer.encode(text, add_special_tokens=False)
    if not encoded.ids:
        raise ValueError("turn has no tokenizer tokens")
    truncated = len(encoded.ids) > SIDE_TOKENS
    retained = text[: encoded.offsets[SIDE_TOKENS - 1][1]] if truncated else text
    count = len(tokenizer.encode(retained, add_special_tokens=False).ids)
    if not 0 < count <= SIDE_TOKENS:
        raise ValueError("literal prefix does not satisfy frozen side budget")
    return Prefix(
        text=retained,
        full_tokens=len(encoded.ids),
        retained_tokens=count,
        truncated=truncated,
    )


def sigmoid(logit: float) -> float:
    if not math.isfinite(logit):
        raise ValueError("cross-encoder returned a nonfinite logit")
    if logit >= 0:
        return 1 / (1 + math.exp(-logit))
    exp = math.exp(logit)
    return exp / (1 + exp)


class Affinity:
    def __init__(self, snapshot: Path) -> None:
        model = snapshot / "onnx/model.onnx"
        tokenizer = snapshot / "tokenizer.json"
        if (
            hashlib.sha256(model.read_bytes()).hexdigest() != MODEL_SHA
            or hashlib.sha256(tokenizer.read_bytes()).hexdigest() != TOKENIZER_SHA
        ):
            raise ValueError("installed reranker assets differ from protocol")
        self.manifest = Manifest(
            runtime=ort.__version__, tokenizer_runtime=tokenizers.__version__
        )
        self.tokenizer = Tokenizer.from_file(str(tokenizer))
        self.tokenizer.no_truncation()
        self.tokenizer.no_padding()
        if self.tokenizer.num_special_tokens_to_add(is_pair=True) != 4:
            raise ValueError("pair tokenizer special-token contract changed")
        options = ort.SessionOptions()
        options.intra_op_num_threads = options.inter_op_num_threads = 1
        self.session = ort.InferenceSession(
            str(model), options, providers=["CPUExecutionProvider"]
        )

    def score(self, left: Prefix, right: Prefix) -> tuple[float, float]:
        encoded = self.tokenizer.encode(left.text, right.text)
        if len(encoded.ids) > PAIR_TOKENS:
            raise ValueError("pair exceeds frozen token budget")
        ids = np.asarray([encoded.ids], dtype=np.int64)
        raw = np.asarray(
            self.session.run(
                ["logits"],
                {
                    "input_ids": ids,
                    "attention_mask": np.ones_like(ids),
                },
            )[0]
        )
        if raw.shape != (1, 1):
            raise ValueError("cross-encoder output shape changed")
        logit = float(raw[0, 0])
        return logit, sigmoid(logit)
