"""Versioned binary text-to-vector contract; never accepts database or file access."""

from __future__ import annotations

import struct
from dataclasses import dataclass

REVISION = "751bff37182d3f1213fa05d7196b954e230abad9"
RECIPE = f"minilm-l6-v2:{REVISION}:onnx-f32:mean-mask:512:l2:v1"
MAX_INPUT_BYTES = 1024 * 1024
MAX_BODY_BYTES = 8 * 1024 * 1024
DIMENSIONS = 384
MAX_INPUTS = 256

REQUEST_MAGIC = b"MSH1"
RESPONSE_MAGIC = b"MSR1"
REQUEST_ID_BYTES = 32
VECTOR_BYTES = DIMENSIONS * 2
RESPONSE_HEADER_BYTES = 4 + REQUEST_ID_BYTES + 4 + 4


class ContractError(ValueError):
    pass


@dataclass(frozen=True)
class Input:
    id: int
    text: str


@dataclass(frozen=True)
class Batch:
    request_id: bytes
    inputs: list[Input]


def decode_batch(body: bytes) -> Batch:
    """Parse the bounded request frame, rejecting every malformed shape."""
    if len(body) < 6 + REQUEST_ID_BYTES + 4 or body[:4] != REQUEST_MAGIC:
        raise ContractError("invalid embedding batch")
    recipe_length = struct.unpack_from("<H", body, 4)[0]
    cursor = 6
    if cursor + recipe_length + REQUEST_ID_BYTES + 4 > len(body):
        raise ContractError("invalid embedding batch")
    recipe = body[cursor : cursor + recipe_length]
    cursor += recipe_length
    if recipe != RECIPE.encode():
        raise ContractError("unsupported embedding recipe")
    request_id = body[cursor : cursor + REQUEST_ID_BYTES]
    cursor += REQUEST_ID_BYTES
    count = struct.unpack_from("<I", body, cursor)[0]
    cursor += 4
    if not 1 <= count <= MAX_INPUTS:
        raise ContractError("invalid embedding batch")
    inputs: list[Input] = []
    total = 0
    for _ in range(count):
        if cursor + 8 > len(body):
            raise ContractError("invalid embedding batch")
        input_id, length = struct.unpack_from("<II", body, cursor)
        cursor += 8
        if input_id >= MAX_INPUTS or cursor + length > len(body):
            raise ContractError("invalid embedding batch")
        try:
            text = body[cursor : cursor + length].decode("utf-8")
        except UnicodeDecodeError:
            raise ContractError("invalid embedding batch") from None
        cursor += length
        total += length
        inputs.append(Input(input_id, text))
    if cursor != len(body):
        raise ContractError("invalid embedding batch")
    if total > MAX_INPUT_BYTES:
        raise ContractError("input byte limit exceeded")
    if sorted(item.id for item in inputs) != list(range(count)):
        raise ContractError("input IDs must be unique and contiguous")
    return Batch(request_id=request_id, inputs=inputs)


def encode_reply(
    request_id: bytes,
    inference_milliseconds: int,
    vectors: list[tuple[int, bytes]],
) -> bytes:
    """Encode f16 vector payloads already packed little-endian by the caller."""
    if len(request_id) != REQUEST_ID_BYTES or len(vectors) > MAX_INPUTS:
        raise ContractError("invalid embedding reply")
    if sorted(input_id for input_id, _ in vectors) != list(range(len(vectors))):
        raise ContractError("reply IDs must be unique and contiguous")
    body = bytearray(RESPONSE_MAGIC)
    body += request_id
    body += struct.pack("<II", inference_milliseconds, len(vectors))
    for input_id, values in vectors:
        if len(values) != VECTOR_BYTES:
            raise ContractError("invalid embedding vector")
        body += struct.pack("<I", input_id)
        body += values
    return bytes(body)
