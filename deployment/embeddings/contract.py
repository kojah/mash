"""Versioned text-to-vector contract; never accepts database or file access."""

from typing import Self

from pydantic import BaseModel, ConfigDict, Field, model_validator

REVISION = "751bff37182d3f1213fa05d7196b954e230abad9"
RECIPE = f"minilm-l6-v2:{REVISION}:onnx-f32:mean-mask:512:l2:v1"
MAX_INPUT_BYTES = 1024 * 1024
MAX_BODY_BYTES = 8 * 1024 * 1024
DIMENSIONS = 384
MAX_INPUTS = 256


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
    values: list[float] = Field(min_length=DIMENSIONS, max_length=DIMENSIONS)


class Reply(Record):
    recipe: str = RECIPE
    request_id: str
    vectors: list[Vector]
    inference_milliseconds: int = Field(ge=0)
