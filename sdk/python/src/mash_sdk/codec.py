"""Canonical encoding checked against the bundled, Rust-owned JSON Schema.

The Rust ingestion boundary remains authoritative for stored-graph validation.
This client check does not imply that a destination accepted or persisted a batch.
"""

from dataclasses import dataclass
from importlib.resources import files
from math import isfinite
from typing import Literal

import orjson
from jsonschema import Draft202012Validator
from jsonschema.protocols import Validator
from pydantic import BaseModel, JsonValue, TypeAdapter, ValidationError
from pydantic_core import PydanticSerializationError

from ._generated.batch import (
    Batch,
    ChangeDeleteChat,
    ChangeDeleteDocument,
    ChangeDeleteMessage,
    ChangeUpsertChat,
    ChangeUpsertDocument,
    ChangeUpsertMessage,
)


class InvalidBatch(ValueError):
    """A content-free client validation error, safe to surface in logs."""


class Limits(BaseModel):
    batch_bytes: int
    content_bytes: int
    metadata_bytes: int


@dataclass(frozen=True)
class Contract:
    validator: Validator
    limits: Limits

    @classmethod
    def bundled(cls) -> "Contract":
        schema = TypeAdapter(dict[str, JsonValue]).validate_python(
            orjson.loads(
                files("mash_sdk._generated").joinpath("batch.schema.json").read_bytes()
            )
        )
        return cls(
            Draft202012Validator(schema), Limits.model_validate(schema["x-mash-limits"])
        )


def finite(value: JsonValue) -> bool:
    if isinstance(value, float):
        return isfinite(value)
    if isinstance(value, list):
        return all(finite(item) for item in value)
    if isinstance(value, dict):
        return all(finite(item) for item in value.values())
    return True


def encode_batch(batch: Batch, *, contract: Contract | None = None) -> bytes:
    """Serialize deterministically for durable retries and content hashing.

    Reject non-finite numbers instead of allowing orjson to silently turn them
    into null. Validation failures never include source text or credentials.
    """
    contract = contract if contract is not None else Contract.bundled()
    try:
        payload = TypeAdapter(JsonValue).validate_python(
            batch.model_dump(mode="json", warnings="error")
        )
    except (ValidationError, PydanticSerializationError):
        raise InvalidBatch("batch contains invalid JSON values") from None
    if (
        not finite(payload)
        or next(iter(contract.validator.iter_errors(payload)), None) is not None
    ):
        raise InvalidBatch("batch does not match the connector schema")
    try:
        encoded = orjson.dumps(payload, option=orjson.OPT_SORT_KEYS)
    except orjson.JSONEncodeError:
        raise InvalidBatch("batch cannot be represented as JSON") from None
    if len(encoded) > contract.limits.batch_bytes:
        raise InvalidBatch("encoded batch exceeds the connector limit")
    seen: set[tuple[Literal["chat", "message", "document"], str]] = set()
    for mutation in batch.changes:
        match mutation:
            case ChangeUpsertChat() | ChangeDeleteChat():
                kind: Literal["chat", "message", "document"] = "chat"
            case ChangeUpsertMessage() | ChangeDeleteMessage():
                kind = "message"
            case ChangeUpsertDocument() | ChangeDeleteDocument():
                kind = "document"
        if (kind, mutation.id) in seen:
            raise InvalidBatch("duplicate mutation")
        seen.add((kind, mutation.id))
        if isinstance(mutation, ChangeUpsertMessage | ChangeUpsertDocument):
            if len(mutation.content.encode()) > contract.limits.content_bytes:
                raise InvalidBatch("content exceeds the connector limit")
        if isinstance(
            mutation, ChangeUpsertChat | ChangeUpsertMessage | ChangeUpsertDocument
        ):
            if (
                mutation.metadata is not None
                and len(orjson.dumps(mutation.metadata))
                > contract.limits.metadata_bytes
            ):
                raise InvalidBatch("metadata exceeds the connector limit")
    return encoded
