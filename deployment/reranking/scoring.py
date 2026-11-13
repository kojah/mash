"""Cross-encoder scoring that reproduces Mash's local FastEmbed reranker.

Mash compares these logits with ones from its CPU reranker, so tokenization
follows FastEmbed's `load_tokenizer` and `TextRerank::rerank` exactly: pair
encoding with special tokens, longest-first truncation at the tokenizer's
`model_max_length` (capped at 512), batch-longest padding with the model
configuration's pad ID, and the first logit as the score.
"""

from __future__ import annotations

import json
from collections.abc import Sequence
from pathlib import Path
from typing import Protocol

import numpy as np
from tokenizers import AddedToken, Tokenizer

MAX_LENGTH = 512
# Similar lengths share a batch, so short candidates are not padded to a long one.
BATCH_SIZE = 16
MODEL_FILES = [
    "onnx/model.onnx",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "config.json",
]


class Session(Protocol):
    def run(self, output_names: None, input_feed: dict[str, np.ndarray]) -> Sequence[object]: ...


def load_tokenizer(root: Path) -> Tokenizer:
    config = json.loads((root / "config.json").read_text())
    tokenizer_config = json.loads((root / "tokenizer_config.json").read_text())
    special = json.loads((root / "special_tokens_map.json").read_text())
    tokenizer = Tokenizer.from_file(str(root / "tokenizer.json"))
    # FastEmbed pads with the model configuration's ID and the tokenizer's token.
    tokenizer.enable_padding(
        pad_id=int(config.get("pad_token_id") or 0),
        pad_token=str(tokenizer_config["pad_token"]),
    )
    tokenizer.enable_truncation(
        max_length=min(MAX_LENGTH, int(tokenizer_config["model_max_length"]))
    )
    for value in special.values():
        if isinstance(value, str):
            # FastEmbed's Rust AddedToken default normalizes plain entries.
            tokenizer.add_special_tokens([AddedToken(value, special=True, normalized=True)])
        else:
            tokenizer.add_special_tokens(
                [
                    AddedToken(
                        value["content"],
                        special=True,
                        single_word=value["single_word"],
                        lstrip=value["lstrip"],
                        rstrip=value["rstrip"],
                        normalized=value["normalized"],
                    )
                ]
            )
    return tokenizer


def score(session: Session, tokenizer: Tokenizer, query: str, texts: Sequence[str]) -> list[float]:
    """Return one raw logit per text, in input order."""
    encodings = tokenizer.encode_batch([(query, text) for text in texts], add_special_tokens=True)
    lengths = [sum(item.attention_mask) for item in encodings]
    order = sorted(range(len(texts)), key=lambda index: lengths[index])
    scores = [0.0] * len(texts)
    for start in range(0, len(order), BATCH_SIZE):
        indices = order[start : start + BATCH_SIZE]
        # Encoding the batch together pads it to its own longest pair.
        batch = tokenizer.encode_batch(
            [(query, texts[index]) for index in indices], add_special_tokens=True
        )
        feed = {
            "input_ids": np.asarray([item.ids for item in batch], dtype=np.int64),
            "attention_mask": np.asarray([item.attention_mask for item in batch], dtype=np.int64),
        }
        logits = np.asarray(session.run(None, feed)[0], dtype=np.float32)
        if logits.shape != (len(indices), 1) or not np.isfinite(logits).all():
            raise ValueError("invalid model output")
        for index, value in zip(indices, logits[:, 0], strict=True):
            scores[index] = float(value)
    return scores
