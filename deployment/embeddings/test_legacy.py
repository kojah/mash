"""Legacy JSON compatibility regressions; no models or GPU work."""

import json

import pytest
from pydantic import ValidationError

from contract import DIMENSIONS, MAX_INPUT_BYTES, MAX_INPUTS, RECIPE
from legacy import Batch, Input, decode_legacy_batch, encode_legacy_reply


def test_legacy_batch_accepts_unicode_and_preserves_input_identity() -> None:
    batch = Batch(
        recipe=RECIPE,
        request_id="a" * 64,
        inputs=[Input(id=1, text="東京"), Input(id=0, text="hello")],
    )
    assert [(item.id, item.text) for item in batch.inputs] == [(1, "東京"), (0, "hello")]


@pytest.mark.parametrize("ids", [[], [0, 0], [1], [0, 2], list(range(MAX_INPUTS + 1))])
def test_legacy_batch_rejects_missing_duplicate_or_out_of_bounds_ids(ids: list[int]) -> None:
    with pytest.raises(ValidationError):
        Batch(recipe=RECIPE, request_id="b" * 64, inputs=[Input(id=i, text="x") for i in ids])


def test_legacy_batch_byte_limit_counts_utf8_bytes() -> None:
    with pytest.raises(ValidationError, match="byte limit"):
        Batch(
            recipe=RECIPE,
            request_id="c" * 64,
            inputs=[Input(id=0, text="é" * (MAX_INPUT_BYTES // 2 + 1))],
        )


def test_legacy_batch_rejects_unknown_recipe() -> None:
    with pytest.raises(ValidationError, match="recipe"):
        Batch(recipe="another-model", request_id="d" * 64, inputs=[Input(id=0, text="x")])


def test_legacy_reply_keeps_the_previous_json_shape() -> None:
    request_id = "e" * 64
    values = [1.0] + [0.0] * 383
    decoded_id, inputs = decode_legacy_batch(
        json.dumps(
            {
                "recipe": RECIPE,
                "request_id": request_id,
                "inputs": [{"id": 0, "text": "hello"}],
            }
        ).encode()
    )
    assert decoded_id == request_id
    assert [(item.id, item.text) for item in inputs] == [(0, "hello")]
    reply = json.loads(encode_legacy_reply(request_id, 7, [(0, values)]))
    assert reply["recipe"] == RECIPE
    assert reply["request_id"] == request_id
    assert reply["inference_milliseconds"] == 7
    assert reply["vectors"] == [{"id": 0, "values": values}]
    assert len(values) == DIMENSIONS
