"""Offline singleton inference using explicitly supplied installed MiniLM assets."""
from dataclasses import dataclass
import hashlib
import importlib.metadata
from pathlib import Path
import time

import numpy as np
from numpy.typing import NDArray
import onnxruntime as ort
from tokenizers import Tokenizer

from data import normalized


def quantized_direction(vector: NDArray[np.float32]) -> NDArray[np.float64]:
    if vector.shape != (384,) or not np.isfinite(vector).all():
        raise ValueError("invalid local embedding")
    maximum = np.max(np.abs(vector))
    if maximum <= np.finfo(np.float32).eps:
        raise ValueError("zero local embedding")
    scaled = vector * (np.float32(127) / maximum)
    # Rust rounds ties away from zero; np.round uses ties-to-even.
    rounded = np.copysign(np.floor(np.abs(scaled) + np.float32(0.5)), scaled)
    return normalized(np.clip(rounded, -127, 127).astype(np.int8).astype(np.float64))


@dataclass
class Statistics:
    model_sha256: str
    tokenizer_sha256: str
    onnxruntime_version: str
    tokenizers_version: str
    cpu_threads: int = 1
    batch_size: int = 1
    calls: int = 0
    cache_hits: int = 0
    inference_ms: float = 0
    tokenization_ms: float = 0
    maximum_input_tokens: int = 0


class Encoder:
    def __init__(self, snapshot: Path):
        model = snapshot / "onnx/model_quantized.onnx"
        tokenizer = snapshot / "tokenizer.json"
        if not model.is_file() or not tokenizer.is_file():
            raise ValueError("explicit installed local assets are required; no downloads")
        if model.stat().st_size > 128 * 1024 * 1024:
            raise ValueError("unexpected model size for this MiniLM control")
        self.statistics = Statistics(hashlib.sha256(model.read_bytes()).hexdigest(),
                                     hashlib.sha256(tokenizer.read_bytes()).hexdigest(),
                                     importlib.metadata.version("onnxruntime"),
                                     importlib.metadata.version("tokenizers"))
        self.tokenizer = Tokenizer.from_file(str(tokenizer))
        self.tokenizer.no_truncation()
        self.tokenizer.no_padding()
        options = ort.SessionOptions()
        options.intra_op_num_threads = 1
        options.inter_op_num_threads = 1
        self.session = ort.InferenceSession(str(model), options, providers=["CPUExecutionProvider"])
        if {i.name for i in self.session.get_inputs()} != {"input_ids", "attention_mask", "token_type_ids"}:
            raise ValueError("unexpected local model inputs")
        if [o.name for o in self.session.get_outputs()] != ["last_hidden_state"]:
            raise ValueError("unexpected local model outputs")
        self.cache: dict[str, NDArray[np.float64]] = {}

    def encode(self, text: str) -> NDArray[np.float64]:
        if text in self.cache:
            self.statistics.cache_hits += 1
            return self.cache[text]
        if len(self.cache) >= 1024 or len(text.encode()) > 64 * 1024:
            raise ValueError("bounded context inference input cap exceeded")
        start = time.perf_counter()
        encoding = self.tokenizer.encode(text)
        self.statistics.tokenization_ms += (time.perf_counter() - start) * 1000
        if not 2 <= len(encoding.ids) <= 512:
            raise ValueError("context exceeds model window; do not silently truncate")
        self.statistics.maximum_input_tokens = max(self.statistics.maximum_input_tokens, len(encoding.ids))
        inputs = {"input_ids": np.asarray([encoding.ids], dtype=np.int64),
                  "attention_mask": np.asarray([encoding.attention_mask], dtype=np.int64),
                  "token_type_ids": np.asarray([encoding.type_ids], dtype=np.int64)}
        start = time.perf_counter()
        hidden = self.session.run(["last_hidden_state"], inputs)[0]
        self.statistics.inference_ms += (time.perf_counter() - start) * 1000
        if hidden.shape != (1, len(encoding.ids), 384) or not np.isfinite(hidden).all():
            raise ValueError("invalid local model output")
        # No padding in singleton inference: all attended tokens are included.
        pooled = hidden[0].mean(axis=0, dtype=np.float32)
        self.cache[text] = quantized_direction(pooled)
        self.statistics.calls += 1
        return self.cache[text]
