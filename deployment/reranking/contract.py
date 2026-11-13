"""Versioned binary query-and-documents-to-scores contract; no database or file access."""

from __future__ import annotations

import struct
from dataclasses import dataclass

import zstandard

REPOSITORY = "jinaai/jina-reranker-v1-turbo-en"
# Mash's installed reranker revision; the client refuses a recipe for another one.
REVISION = "b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2"
RECIPE = f"jina-reranker-v1-turbo-en:{REVISION}:onnx-f32:pair-512-longest-first:logit0:v1"
MAX_QUERY_BYTES = 16 * 1024
# A chat search reranks at most ten threads of six messages.
MAX_DOCUMENTS = 64
MAX_DOCUMENT_BYTES = 1024 * 1024
MAX_BODY_BYTES = 2 * 1024 * 1024
# Raw cross-encoder logits for this model stay far inside this range; anything
# outside it is a broken model or transport, not a relevance judgement.
MAX_SCORE_MAGNITUDE = 1000.0

REQUEST_MAGIC = b"MRK1"
RESPONSE_MAGIC = b"MRR1"
REQUEST_ID_BYTES = 32


class ContractError(ValueError):
    pass


@dataclass(frozen=True)
class Document:
    id: int
    text: str


@dataclass(frozen=True)
class Batch:
    """A query and its documents. No documents means a warm-up request."""

    request_id: bytes
    query: str
    documents: list[Document]


def decode_text(body: bytes, cursor: int, length: int) -> str:
    if cursor + length > len(body):
        raise ContractError("invalid reranking batch")
    try:
        return body[cursor : cursor + length].decode("utf-8")
    except UnicodeDecodeError:
        raise ContractError("invalid reranking batch") from None


def decode_batch(body: bytes) -> Batch:
    """Parse the bounded request frame, rejecting every malformed shape."""
    if len(body) < 6 + REQUEST_ID_BYTES + 8 or body[:4] != REQUEST_MAGIC:
        raise ContractError("invalid reranking batch")
    recipe_length = struct.unpack_from("<H", body, 4)[0]
    cursor = 6
    if cursor + recipe_length + REQUEST_ID_BYTES + 8 > len(body):
        raise ContractError("invalid reranking batch")
    if body[cursor : cursor + recipe_length] != RECIPE.encode():
        raise ContractError("unsupported reranking recipe")
    cursor += recipe_length
    request_id = body[cursor : cursor + REQUEST_ID_BYTES]
    cursor += REQUEST_ID_BYTES
    query_length = struct.unpack_from("<I", body, cursor)[0]
    cursor += 4
    if query_length > MAX_QUERY_BYTES:
        raise ContractError("query byte limit exceeded")
    query = decode_text(body, cursor, query_length)
    cursor += query_length
    if cursor + 4 > len(body):
        raise ContractError("invalid reranking batch")
    count = struct.unpack_from("<I", body, cursor)[0]
    cursor += 4
    if count > MAX_DOCUMENTS or (count == 0) != (query_length == 0):
        raise ContractError("invalid reranking batch")
    documents: list[Document] = []
    total = 0
    for _ in range(count):
        if cursor + 8 > len(body):
            raise ContractError("invalid reranking batch")
        document_id, length = struct.unpack_from("<II", body, cursor)
        cursor += 8
        if document_id >= MAX_DOCUMENTS:
            raise ContractError("invalid reranking batch")
        documents.append(Document(document_id, decode_text(body, cursor, length)))
        cursor += length
        total += length
    if cursor != len(body):
        raise ContractError("invalid reranking batch")
    if total > MAX_DOCUMENT_BYTES:
        raise ContractError("document byte limit exceeded")
    if sorted(item.id for item in documents) != list(range(count)):
        raise ContractError("document IDs must be unique and contiguous")
    return Batch(request_id=request_id, query=query, documents=documents)


def decompress_request(data: bytes) -> bytes:
    """Decode a zstd HTTP body while bounding the decompressed size."""
    try:
        return zstandard.ZstdDecompressor().decompress(data, max_output_size=MAX_BODY_BYTES)
    except zstandard.ZstdError:
        raise ContractError("invalid compressed reranking batch") from None


def encode_reply(
    request_id: bytes,
    inference_milliseconds: int,
    scores: list[tuple[int, float]],
) -> bytes:
    """Encode one little-endian f32 logit per document, keyed by document ID."""
    if len(request_id) != REQUEST_ID_BYTES or len(scores) > MAX_DOCUMENTS:
        raise ContractError("invalid reranking reply")
    if sorted(document_id for document_id, _ in scores) != list(range(len(scores))):
        raise ContractError("reply IDs must be unique and contiguous")
    body = bytearray(RESPONSE_MAGIC)
    body += request_id
    body += struct.pack("<II", inference_milliseconds, len(scores))
    for document_id, score in scores:
        # Also rejects NaN, which fails every comparison.
        if not abs(score) <= MAX_SCORE_MAGNITUDE:
            raise ContractError("invalid reranking score")
        body += struct.pack("<If", document_id, score)
    return bytes(body)
