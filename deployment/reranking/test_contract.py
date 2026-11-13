"""Boundary regressions without downloads or billable GPU work."""

import os
import struct
from pathlib import Path

import numpy as np
import pytest
import zstandard

from contract import (
    MAX_DOCUMENT_BYTES,
    MAX_DOCUMENTS,
    MAX_QUERY_BYTES,
    RECIPE,
    REQUEST_ID_BYTES,
    REVISION,
    ContractError,
    decode_batch,
    decompress_request,
    encode_reply,
)
from scoring import BATCH_SIZE, load_tokenizer, score

REQUEST_ID = b"a" * REQUEST_ID_BYTES


def pack(
    query: str,
    documents: list[tuple[int, str]],
    *,
    recipe: str = RECIPE,
    body_id: bytes = REQUEST_ID,
) -> bytes:
    encoded_recipe = recipe.encode()
    encoded_query = query.encode()
    body = bytearray(b"MRK1")
    body += struct.pack("<H", len(encoded_recipe))
    body += encoded_recipe
    body += body_id
    body += struct.pack("<I", len(encoded_query))
    body += encoded_query
    body += struct.pack("<I", len(documents))
    for document_id, text in documents:
        encoded = text.encode()
        body += struct.pack("<II", document_id, len(encoded))
        body += encoded
    return bytes(body)


def test_batch_accepts_unicode_and_preserves_document_identity() -> None:
    batch = decode_batch(pack("寿司 recipe", [(1, "東京"), (0, "hello")]))
    assert batch.request_id == REQUEST_ID
    assert batch.query == "寿司 recipe"
    assert [(item.id, item.text) for item in batch.documents] == [(1, "東京"), (0, "hello")]


def test_empty_batch_is_a_warm_up_only_without_a_query() -> None:
    warm_up = decode_batch(pack("", []))
    assert (warm_up.query, warm_up.documents) == ("", [])
    with pytest.raises(ContractError):
        decode_batch(pack("query", []))
    with pytest.raises(ContractError):
        decode_batch(pack("", [(0, "document")]))


@pytest.mark.parametrize("ids", [[0, 0], [1], [0, 2], list(range(MAX_DOCUMENTS + 1))])
def test_batch_rejects_missing_duplicate_or_out_of_bounds_ids(ids: list[int]) -> None:
    with pytest.raises(ContractError):
        decode_batch(pack("q", [(document_id, "x") for document_id in ids]))


def test_byte_limits_count_utf8_bytes() -> None:
    with pytest.raises(ContractError, match="document byte limit"):
        decode_batch(pack("q", [(0, "é" * (MAX_DOCUMENT_BYTES // 2 + 1))]))
    with pytest.raises(ContractError, match="query byte limit"):
        decode_batch(pack("é" * (MAX_QUERY_BYTES // 2 + 1), [(0, "x")]))


def test_batch_rejects_unknown_recipe_and_malformed_frames() -> None:
    with pytest.raises(ContractError, match="recipe"):
        decode_batch(pack("q", [(0, "x")], recipe="another-model"))
    valid = pack("q", [(0, "x")])
    for malformed in [b"", valid[:5], valid[:-1], valid + b"x", b"MSH1" + valid[4:]]:
        with pytest.raises(ContractError):
            decode_batch(malformed)
    with pytest.raises(ContractError):
        decode_batch(pack("q", [(0, "x")])[:-1] + b"\xff")


def test_recipe_pins_the_reranker_revision() -> None:
    assert REVISION in RECIPE
    assert len(REVISION) == 40


def test_compressed_requests_round_trip_and_reject_malformed_input() -> None:
    valid = pack("query", [(0, "hello"), (1, "東京")])
    compressed = zstandard.ZstdCompressor(level=3).compress(valid)
    assert decompress_request(compressed) == valid
    assert decode_batch(decompress_request(compressed)).documents[1].text == "東京"
    with pytest.raises(ContractError):
        decompress_request(b"not a zstd frame")


def test_reply_is_fixed_width_f32_and_bounded() -> None:
    reply = encode_reply(REQUEST_ID, 7, [(1, -2.5), (0, 1.25)])
    assert reply[:4] == b"MRR1"
    assert reply[4:36] == REQUEST_ID
    assert struct.unpack_from("<II", reply, 36) == (7, 2)
    assert struct.unpack_from("<IfIf", reply, 44) == (1, -2.5, 0, 1.25)
    assert len(reply) == 44 + 2 * 8
    assert encode_reply(REQUEST_ID, 0, []) == b"MRR1" + REQUEST_ID + struct.pack("<II", 0, 0)
    for invalid in [float("nan"), float("inf"), 1e300, 1001.0]:
        with pytest.raises(ContractError):
            encode_reply(REQUEST_ID, 7, [(0, invalid)])
    with pytest.raises(ContractError):
        encode_reply(REQUEST_ID, 7, [(1, 0.0)])


class Recorder:
    """Stands in for an ONNX session: one logit per pair, its token count."""

    def __init__(self) -> None:
        self.batches: list[int] = []

    def run(self, output_names: None, input_feed: dict[str, np.ndarray]) -> list[np.ndarray]:
        mask = input_feed["attention_mask"]
        self.batches.append(mask.shape[0])
        return [mask.sum(axis=1, keepdims=True).astype(np.float32)]


def tokenizer_root() -> Path | None:
    root = os.environ.get("MASH_TEST_RERANKER_DIR")
    return Path(root) if root else None


@pytest.mark.skipif(tokenizer_root() is None, reason="set MASH_TEST_RERANKER_DIR")
def test_scores_keep_input_order_across_length_sorted_batches() -> None:
    root = tokenizer_root()
    assert root is not None
    tokenizer = load_tokenizer(root)
    texts = [("word " * (index % 7 + 1)).strip() for index in range(BATCH_SIZE * 2 + 3)]
    session = Recorder()
    scores = score(session, tokenizer, "query", texts)
    assert session.batches == [BATCH_SIZE, BATCH_SIZE, 3]
    # <s> query </s></s> text </s>: four specials, one query token, one per word.
    assert scores == [float(5 + index % 7 + 1) for index in range(len(texts))]
    long = score(session, tokenizer, "query", ["word " * 2000])
    assert long == [512.0]


@pytest.mark.skipif(tokenizer_root() is None, reason="set MASH_TEST_RERANKER_DIR")
def test_cpu_model_ranks_the_relevant_document_first() -> None:
    import onnxruntime as ort

    root = tokenizer_root()
    assert root is not None
    session = ort.InferenceSession(
        str(root / "onnx/model.onnx"), providers=["CPUExecutionProvider"]
    )
    scores = score(
        session,
        load_tokenizer(root),
        "how do I rotate a database password",
        [
            "The cat slept on the warm windowsill all afternoon.",
            "Rotate the Postgres password with ALTER ROLE and update the secret.",
        ],
    )
    assert scores[1] > scores[0]
