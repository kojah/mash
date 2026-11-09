"""Boundary regressions without models, downloads, or billable GPU work."""

import struct

import pytest
import zstandard

from contract import (
    DIMENSIONS,
    MAX_INPUT_BYTES,
    MAX_INPUTS,
    RECIPE,
    REQUEST_ID_BYTES,
    VECTOR_BYTES,
    ContractError,
    decode_batch,
    decompress_request,
    encode_reply,
)

REQUEST_ID = b"a" * REQUEST_ID_BYTES


def pack(
    inputs: list[tuple[int, str]],
    *,
    recipe: str = RECIPE,
    body_id: bytes = REQUEST_ID,
) -> bytes:
    encoded_recipe = recipe.encode()
    body = bytearray(b"MSH1")
    body += struct.pack("<H", len(encoded_recipe))
    body += encoded_recipe
    body += body_id
    body += struct.pack("<I", len(inputs))
    for input_id, text in inputs:
        encoded = text.encode()
        body += struct.pack("<II", input_id, len(encoded))
        body += encoded
    return bytes(body)


def test_batch_accepts_unicode_and_preserves_input_identity() -> None:
    batch = decode_batch(pack([(1, "東京"), (0, "hello")]))
    assert batch.request_id == REQUEST_ID
    assert [(item.id, item.text) for item in batch.inputs] == [(1, "東京"), (0, "hello")]


@pytest.mark.parametrize("ids", [[], [0, 0], [1], [0, 2], list(range(MAX_INPUTS + 1))])
def test_batch_rejects_missing_duplicate_or_out_of_bounds_ids(ids: list[int]) -> None:
    with pytest.raises(ContractError):
        decode_batch(pack([(input_id, "x") for input_id in ids]))


def test_batch_byte_limit_counts_utf8_bytes() -> None:
    with pytest.raises(ContractError, match="byte limit"):
        decode_batch(pack([(0, "é" * (MAX_INPUT_BYTES // 2 + 1))]))


def test_batch_rejects_unknown_recipe_and_malformed_frames() -> None:
    with pytest.raises(ContractError, match="recipe"):
        decode_batch(pack([(0, "x")], recipe="another-model"))
    valid = pack([(0, "x")])
    for malformed in [b"", valid[:5], valid + b"x", b"XSH1" + valid[4:]]:
        with pytest.raises(ContractError):
            decode_batch(malformed)


def test_non_binary_bodies_are_rejected() -> None:
    with pytest.raises(ContractError):
        decode_batch(b'{"recipe":"legacy","request_id":"' + b"a" * 64 + b'","inputs":[]}')


def test_compressed_requests_round_trip_and_reject_malformed_input() -> None:
    valid = pack([(0, "hello"), (1, "東京")])
    compressed = zstandard.ZstdCompressor(level=3).compress(valid)
    assert decompress_request(compressed) == valid
    assert decode_batch(decompress_request(compressed)).inputs[1].text == "東京"
    with pytest.raises(ContractError):
        decompress_request(b"not a zstd frame")


def test_reply_is_fixed_width_f16_and_bounded() -> None:
    values = struct.pack("<e", 1.0) * (DIMENSIONS - 1) + struct.pack("<e", 0.0)
    assert len(values) == VECTOR_BYTES
    reply = encode_reply(REQUEST_ID, 7, [(0, values)])
    assert reply[:4] == b"MSR1"
    assert reply[4:36] == REQUEST_ID
    assert int.from_bytes(reply[36:40], "little") == 7
    assert int.from_bytes(reply[40:44], "little") == 1
    assert reply[44:48] == struct.pack("<I", 0)
    assert reply[48:] == values
    with pytest.raises(ContractError):
        encode_reply(REQUEST_ID, 7, [(0, b"short")])
    with pytest.raises(ContractError):
        encode_reply(REQUEST_ID, 7, [(1, values)])
