"""Private receipt decoding before durable connector state can advance."""

from importlib.resources import files

import orjson
from jsonschema import Draft202012Validator, FormatChecker
from pydantic import JsonValue, TypeAdapter, ValidationError

from ._generated.receipt import Receipt

MAX_RECEIPT_BYTES = 16 * 1024


class InvalidReceipt(ValueError):
    """A malformed or unsupported receipt; never echoes the response body."""


def decode_receipt(payload: bytes) -> Receipt:
    """Required wire fields must exist, even when generated models have defaults."""
    if len(payload) > MAX_RECEIPT_BYTES:
        raise InvalidReceipt("receipt exceeds the response limit")
    try:
        value = TypeAdapter(JsonValue).validate_python(orjson.loads(payload))
        schema = TypeAdapter(dict[str, JsonValue]).validate_python(
            orjson.loads(
                files("mash_sdk._generated")
                .joinpath("receipt.schema.json")
                .read_bytes()
            )
        )
        checker = FormatChecker()
        # Schemars uses this format for Rust u64. JSON Schema's integer type
        # alone does not enforce the representable range.
        checker.checks("uint64")(
            lambda number: (
                isinstance(number, int)
                and not isinstance(number, bool)
                and 0 <= number < 1 << 64
            )
        )
        validator = Draft202012Validator(schema, format_checker=checker)
        if next(validator.iter_errors(value), None) is not None:
            raise InvalidReceipt("receipt does not match the connector schema")
        return Receipt.model_validate(value)
    except (orjson.JSONDecodeError, ValidationError):
        raise InvalidReceipt("receipt is not valid connector JSON") from None
