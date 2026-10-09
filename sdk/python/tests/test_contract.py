"""Offline SDK checks plus explicitly enabled real Rust decoder conformance."""

import os
import subprocess
import unittest
from pathlib import Path

import orjson
from pydantic import ValidationError

from mash_sdk import (
    Attachment,
    Batch,
    Changes,
    InvalidBatch,
    Object,
    Role,
    encode_batch,
)
from mash_sdk.codec import Contract
from tests.objects import channel, message, note


def batch(changes: Changes) -> Batch:
    return changes.to_batch(
        source_id="test-source", batch_id="batch-1", expected_revision=0
    )


class ContractTests(unittest.TestCase):
    def test_rust_owned_byte_budgets_apply_to_unicode_content_and_metadata(
        self,
    ) -> None:
        contract = Contract.bundled()
        oversized = "é" * (contract.limits.content_bytes // 2 + 1)
        with self.assertRaises(InvalidBatch):
            encode_batch(
                batch(Changes.upsert(note(id="note", content=oversized))),
                contract=contract,
            )
        oversized_metadata = "é" * (contract.limits.metadata_bytes // 2 + 1)
        with self.assertRaises(InvalidBatch):
            encode_batch(
                batch(
                    Changes.upsert(
                        channel(id="chat", metadata={"text": oversized_metadata})
                    )
                ),
                contract=contract,
            )

    def test_objects_carry_roles_and_preserve_metadata_and_attachments(
        self,
    ) -> None:
        changes = Changes.upsert(
            channel(id="channel", metadata={"workspace": {"id": "work"}}),
            message(
                id="message", parent="channel", content="A reply", reply_to="parent"
            ),
            note(
                id="note",
                title="Design",
                content="Canonical notes",
                metadata={"tags": ["storage"]},
                attachments=[
                    Attachment(
                        id="file",
                        name="image.png",
                        media_type="image/png",
                        url="https://example.com/file",
                    )
                ],
            ),
        )
        encoded = encode_batch(batch(changes))
        decoded = Batch.model_validate_json(encoded)
        self.assertEqual(decoded, batch(changes))
        self.assertEqual(decoded.changes[2].op, "upsert-object")
        upserted = decoded.changes[2]
        assert isinstance(upserted, Object)
        self.assertEqual(upserted.role, Role.document)
        self.assertIn(b'"reply_to":"parent"', encoded)
        with self.assertRaises(ValidationError):
            channel(id="chat", chat_id="invented")
        for kind in ("Chat", "message", "slack:"):
            with self.assertRaises(ValidationError):
                note(id="n", content="x", kind=kind)

    def test_changes_capture_a_copy_and_encoding_is_deterministic(self) -> None:
        record = note(id="note", content="original", metadata={"b": 2, "a": 1})
        changes = Changes.upsert(record)
        record.content = "later edit"
        original = encode_batch(batch(changes))
        equivalent = encode_batch(
            batch(
                Changes.upsert(
                    note(id="note", content="original", metadata={"a": 1, "b": 2})
                )
            )
        )
        self.assertEqual(original, equivalent)
        self.assertNotIn(b"later edit", original)

    def test_deletions_are_explicit_and_share_one_identity_namespace(self) -> None:
        changes = Changes.upsert(
            channel(id="a"), note(id="b", content="note")
        ) + Changes.delete("old")
        self.assertEqual(len(batch(changes).changes), 3)
        encode_batch(batch(changes))
        for duplicate in (
            Changes.upsert(channel(id="same")) + Changes.delete("same"),
            Changes.upsert(channel(id="same"), note(id="same", content="note")),
        ):
            with self.assertRaises(InvalidBatch):
                encode_batch(batch(duplicate))
        with self.assertRaises(ValidationError):
            batch(Changes.empty())

    def test_schema_enforces_full_id_pattern_and_strict_types(self) -> None:
        # Some regex engines accept a terminal newline at `$`; the schema's `not`
        # clause is required in addition to the generated Pydantic constraint.
        value = batch(Changes.upsert(note(id="note", content="text")))
        value.source_id = "source\n"
        with self.assertRaises(InvalidBatch):
            encode_batch(value)
        with self.assertRaises(ValidationError):
            Batch.model_validate(
                {
                    "source_id": "test",
                    "batch_id": "batch",
                    "expected_revision": "0",
                    "changes": [{"op": "delete-object", "id": "chat"}],
                }
            )

    def test_nonfinite_numbers_and_mutated_types_fail_without_echoing_contents(
        self,
    ) -> None:
        for invalid in (float("nan"), float("inf"), float("-inf")):
            value = batch(
                Changes.upsert(
                    note(
                        id="note", content="PRIVATE-CANARY", metadata={"score": invalid}
                    )
                )
            )
            with self.assertRaises(InvalidBatch) as raised:
                encode_batch(value)
            self.assertNotIn("PRIVATE-CANARY", str(raised.exception))
        value = batch(Changes.upsert(note(id="note", content="text")))
        value.expected_revision = -1
        with self.assertRaises(InvalidBatch):
            encode_batch(value)

    def test_python_payloads_are_accepted_by_the_actual_rust_decoder(self) -> None:
        binary = os.environ.get("MASH_CONNECTOR_CONTRACT_BINARY")
        if binary is None:
            self.skipTest(
                "set MASH_CONNECTOR_CONTRACT_BINARY for cross-language validation"
            )
        if not Path(binary).is_file():
            self.fail("configured Rust contract binary is missing")
        repo = Path(__file__).resolve().parents[3]
        fixture = Batch.model_validate_json(
            (repo / "fixtures/external-connector-batch.json").read_bytes()
        )
        cases = [
            fixture,
            batch(
                Changes.upsert(
                    channel(id="channel"),
                    message(id="message", parent="channel", content="hello"),
                    note(
                        id="note",
                        content="note",
                        metadata={"nested": [1, True, None, "text"]},
                    ),
                )
                + Changes.delete("older-chat")
                + Changes.delete("older-message")
                + Changes.delete("older-note")
            ),
        ]
        for value in cases:
            with self.subTest(source=value.source_id):
                payload = encode_batch(value)
                result = subprocess.run(
                    [binary, "validate"],
                    input=payload,
                    capture_output=True,
                    check=False,
                    timeout=10,
                )
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual(result.stdout.strip(), b"accepted")
                self.assertEqual(orjson.loads(payload)["schema_version"], 2)
