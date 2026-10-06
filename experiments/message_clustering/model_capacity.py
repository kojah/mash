"""Explicit model capabilities and lossless model-token-aware input construction."""

import re
from pathlib import Path

from pydantic import Field, model_validator
from tokenizers import Tokenizer

from corpus import Record
from voyage_vectors import Model


class Capacity(Record):
    model: Model
    dimensions: int = 1024
    context_tokens: int = 32000
    input_tokens: int = 31992
    batch_tokens: int = 100000
    maximum_batch_inputs: int = 64
    tokenizer_sha256: str
    task: str = "provider_default"

    @model_validator(mode="after")
    def valid(self) -> "Capacity":
        if self.dimensions not in (256, 512, 1024, 2048):
            raise ValueError("unsupported Voyage dimensions")
        if not 16 <= self.input_tokens <= self.context_tokens <= 32000:
            raise ValueError("invalid model context/input token budget")
        if self.batch_tokens < self.input_tokens or self.maximum_batch_inputs < 1:
            raise ValueError("batch capacity cannot contain an input")
        return self


class VectorSpace(Record):
    model: Model
    dimensions: int
    task: str
    tokenizer_sha256: str


def vector_space(capacity: Capacity) -> VectorSpace:
    return VectorSpace(
        model=capacity.model,
        dimensions=capacity.dimensions,
        task=capacity.task,
        tokenizer_sha256=capacity.tokenizer_sha256,
    )


class LocalCapacity(Record):
    dimensions: int = Field(gt=0)
    context_tokens: int = Field(gt=0)
    pad_token_id: int = Field(ge=0)


class TokenBudget:
    def __init__(self, path: Path, capacity: Capacity) -> None:
        import hashlib

        if hashlib.sha256(path.read_bytes()).hexdigest() != capacity.tokenizer_sha256:
            raise ValueError("model tokenizer hash mismatch")
        self.capacity = capacity
        self.tokenizer = Tokenizer.from_file(str(path))
        self.tokenizer.no_truncation()
        self.tokenizer.no_padding()

    def count(self, text: str) -> int:
        return len(self.tokenizer.encode(text).ids)

    def parts(self, text: str) -> tuple[str, ...]:
        """Keep all source characters and prefer late structural boundaries."""
        remaining = text
        result = []
        limit = self.capacity.input_tokens
        while remaining:
            encoded = self.tokenizer.encode(remaining)
            if len(encoded.ids) <= limit:
                result.append(remaining)
                break
            offsets = tuple((a, b) for a, b in encoded.offsets if b > a)
            end = offsets[min(limit - 5, len(offsets) - 1)][1]
            while end > 0 and self.count(remaining[:end]) > limit:
                end -= max(1, end // 100)
            if end <= 0:
                raise ValueError("a source character cannot fit the model budget")
            boundaries = [m.end() for m in re.finditer(r"\n\n|\n", remaining[:end])]
            if boundaries and boundaries[-1] >= end // 2:
                candidate = boundaries[-1]
                if self.count(remaining[:candidate]) <= limit:
                    end = candidate
            result.append(remaining[:end])
            remaining = remaining[end:]
        if "".join(result) != text:
            raise ValueError("token splitting dropped source characters")
        return tuple(result)

    def batches(self, texts: tuple[str, ...]) -> tuple[tuple[str, ...], ...]:
        result = []
        current = []
        total = 0
        for text in texts:
            count = self.count(text)
            if not text or count > self.capacity.input_tokens:
                raise ValueError("input exceeds model token budget")
            if current and (
                len(current) >= self.capacity.maximum_batch_inputs
                or total + count > self.capacity.batch_tokens
            ):
                result.append(tuple(current))
                current, total = [], 0
            current.append(text)
            total += count
        if current:
            result.append(tuple(current))
        return tuple(result)
