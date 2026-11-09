"""Previous JSON wire contract, kept while deployed clients migrate to MSH1/MSR1."""

from typing import Self

import orjson
from pydantic import BaseModel, ConfigDict, Field, ValidationError, model_validator

from contract import MAX_INPUT_BYTES, MAX_INPUTS, RECIPE


class Record(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)


class Input(Record):
    id: int = Field(ge=0, lt=MAX_INPUTS)
    text: str = Field(max_length=MAX_INPUT_BYTES)


class Batch(Record):
    recipe: str
    request_id: str = Field(pattern=r"^[0-9a-f]{64}$")
    inputs: list[Input] = Field(min_length=1, max_length=MAX_INPUTS)

    @model_validator(mode="after")
    def check_batch(self) -> Self:
        if self.recipe != RECIPE:
            raise ValueError("unsupported embedding recipe")
        if sorted(item.id for item in self.inputs) != list(range(len(self.inputs))):
            raise ValueError("input IDs must be unique and contiguous")
        if sum(len(item.text.encode("utf-8")) for item in self.inputs) > MAX_INPUT_BYTES:
            raise ValueError("input byte limit exceeded")
        return self


class Vector(Record):
    id: int
    values: list[float] = Field(min_length=384, max_length=384)


class Reply(Record):
    recipe: str = RECIPE
    request_id: str
    vectors: list[Vector]
    inference_milliseconds: int = Field(ge=0)


def decode_legacy_batch(body: bytes) -> tuple[str, list[Input]]:
    batch = Batch.model_validate_json(body)
    return batch.request_id, batch.inputs


def encode_legacy_reply(
    request_id: str,
    inference_milliseconds: int,
    vectors: list[tuple[int, list[float]]],
) -> bytes:
    reply = Reply(
        request_id=request_id,
        vectors=[Vector(id=input_id, values=values) for input_id, values in vectors],
        inference_milliseconds=inference_milliseconds,
    )
    return orjson.dumps(reply.model_dump())


__all__ = [
    "Batch",
    "Input",
    "ValidationError",
    "Vector",
    "decode_legacy_batch",
    "encode_legacy_reply",
]
