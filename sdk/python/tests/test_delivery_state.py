"""Real SQLite restart/ordering tests for the private SDK delivery machinery."""

import sqlite3
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import orjson

from mash_sdk import Changes
from mash_sdk._receipts import InvalidReceipt, decode_receipt
from mash_sdk._state import PendingBatch, PendingExists, StateError, StateStore
from tests.objects import note


def changes(text: str = "First version") -> Changes:
    return Changes.upsert(note(id="note", content=text))


def receipt(
    pending: PendingBatch, *, ready: bool = False, silver_revision: int = 1
) -> bytes:
    return orjson.dumps(
        {
            "schema_version": 2,
            "source_id": pending.source_id,
            "batch_id": pending.batch_id,
            "source_revision": pending.expected_revision + 1,
            "silver_revision": silver_revision,
            "gold_ready": ready,
        }
    )


class DeliveryStateTests(unittest.TestCase):
    def test_payload_identity_corruption_is_rejected_before_delivery(self) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            StateStore(Path(directory) / "state.sqlite") as store,
        ):
            pending = store.prepare(
                changes(), source_id="source", destination_id="local"
            )
            for field, replacement in [
                ("source_id", "wrong-source"),
                ("batch_id", "wrong-batch"),
                ("expected_revision", 9),
            ]:
                altered = orjson.loads(pending.payload)
                altered[field] = replacement
                store.connection.execute(
                    "UPDATE pending SET payload=?", (orjson.dumps(altered),)
                )
                with self.assertRaises(StateError):
                    store.pending(source_id="source", destination_id="local")
            store.connection.execute("UPDATE pending SET payload=?", (pending.payload,))
            self.assertEqual(
                store.pending(source_id="source", destination_id="local"), pending
            )

    def test_corrupt_state_errors_do_not_echo_record_text(self) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            StateStore(Path(directory) / "state.sqlite") as store,
        ):
            store.prepare(changes(), source_id="source", destination_id="local")
            store.connection.execute("UPDATE pending SET payload='PRIVATE-CANARY'")
            with self.assertRaises(StateError) as raised:
                store.pending(source_id="source", destination_id="local")
            self.assertNotIn("PRIVATE-CANARY", str(raised.exception))

    def test_two_writers_cannot_prepare_competing_batches_for_one_scope(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.sqlite"
            with StateStore(path):
                pass

            def prepare() -> PendingBatch | None:
                with StateStore(path) as store:
                    try:
                        return store.prepare(
                            changes(), source_id="source", destination_id="local"
                        )
                    except PendingExists:
                        return None

            with ThreadPoolExecutor(max_workers=2) as executor:
                results = list(executor.map(lambda _: prepare(), range(2)))
            accepted = [result for result in results if result is not None]
            self.assertEqual(len(accepted), 1)
            with StateStore(path) as reopened:
                self.assertEqual(
                    reopened.pending(source_id="source", destination_id="local"),
                    accepted[0],
                )

    def test_acknowledgement_is_atomic_if_checkpoint_update_fails(self) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            StateStore(Path(directory) / "state.sqlite") as store,
        ):
            pending = store.prepare(
                changes(), source_id="source", destination_id="local"
            )
            store.connection.execute(
                "CREATE TRIGGER reject_checkpoint BEFORE UPDATE ON scopes "
                "BEGIN SELECT RAISE(ABORT, 'simulated failure'); END"
            )
            with self.assertRaises(sqlite3.IntegrityError):
                store.acknowledge(
                    receipt(pending), source_id="source", destination_id="local"
                )
            self.assertEqual(
                store.pending(source_id="source", destination_id="local"), pending
            )
            self.assertEqual(
                store.checkpoint(source_id="source", destination_id="local").revision, 0
            )
            store.connection.execute("DROP TRIGGER reject_checkpoint")
            store.acknowledge(
                receipt(pending), source_id="source", destination_id="local"
            )
            self.assertIsNone(store.pending(source_id="source", destination_id="local"))

    def test_lost_response_and_restart_keep_identical_pending_bytes_and_cursor(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.sqlite"
            with StateStore(path) as store:
                pending = store.prepare(
                    changes(),
                    source_id="source",
                    destination_id="local",
                    next_cursor={"position": "page-2"},
                )
                self.assertEqual(
                    store.checkpoint(
                        source_id="source", destination_id="local"
                    ).revision,
                    0,
                )
            with StateStore(path) as reopened:
                self.assertEqual(
                    reopened.pending(source_id="source", destination_id="local"),
                    pending,
                )
                with self.assertRaises(PendingExists):
                    reopened.prepare(
                        changes("Newer version"),
                        source_id="source",
                        destination_id="local",
                    )
                acknowledgement = receipt(pending)
                reopened.acknowledge(
                    acknowledgement, source_id="source", destination_id="local"
                )
                reopened.acknowledge(
                    acknowledgement, source_id="source", destination_id="local"
                )
            with StateStore(path) as reopened:
                checkpoint = reopened.checkpoint(
                    source_id="source", destination_id="local"
                )
                self.assertEqual(checkpoint.revision, 1)
                self.assertEqual(checkpoint.cursor, {"position": "page-2"})
                self.assertIsNotNone(checkpoint.receipt)
                self.assertFalse(
                    checkpoint.receipt.gold_ready if checkpoint.receipt else True
                )
                self.assertIsNone(
                    reopened.pending(source_id="source", destination_id="local")
                )
                next_batch = reopened.prepare(
                    changes("Second version"),
                    source_id="source",
                    destination_id="local",
                )
                reopened.acknowledge(
                    receipt(next_batch, silver_revision=2),
                    source_id="source",
                    destination_id="local",
                )
                self.assertEqual(
                    reopened.checkpoint(
                        source_id="source", destination_id="local"
                    ).cursor,
                    {"position": "page-2"},
                )

    def test_wrong_missing_or_malformed_receipts_do_not_advance_anything(self) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            StateStore(Path(directory) / "state.sqlite") as store,
        ):
            pending = store.prepare(
                changes(), source_id="source", destination_id="shared"
            )
            for field, value in (
                ("source_id", "another-source"),
                ("batch_id", "another-batch"),
                ("source_revision", 9),
            ):
                wrong = orjson.loads(receipt(pending))
                wrong[field] = value
                with self.assertRaises(StateError):
                    store.acknowledge(
                        orjson.dumps(wrong), source_id="source", destination_id="shared"
                    )
            missing = orjson.loads(receipt(pending))
            del missing["schema_version"]
            for raw in (b"broken PRIVATE-CANARY", orjson.dumps(missing)):
                with self.assertRaises(InvalidReceipt) as raised:
                    store.acknowledge(raw, source_id="source", destination_id="shared")
                self.assertNotIn("PRIVATE-CANARY", str(raised.exception))
            self.assertEqual(
                store.pending(source_id="source", destination_id="shared"), pending
            )
            self.assertEqual(
                store.checkpoint(source_id="source", destination_id="shared").revision,
                0,
            )

    def test_sources_and_destinations_have_independent_ordering(self) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            StateStore(Path(directory) / "state.sqlite") as store,
        ):
            local = store.prepare(changes(), source_id="source", destination_id="local")
            shared = store.prepare(
                changes(), source_id="source", destination_id="shared"
            )
            other = store.prepare(changes(), source_id="other", destination_id="local")
            store.acknowledge(
                receipt(local), source_id="source", destination_id="local"
            )
            newer = store.prepare(
                changes("Updated"), source_id="source", destination_id="local"
            )
            self.assertEqual(newer.expected_revision, 1)
            self.assertNotEqual(newer.batch_id, local.batch_id)
            self.assertEqual(
                store.pending(source_id="source", destination_id="shared"), shared
            )
            self.assertEqual(
                store.pending(source_id="other", destination_id="local"), other
            )
            with self.assertRaises(StateError):
                store.acknowledge(
                    receipt(local), source_id="source", destination_id="local"
                )
            self.assertEqual(
                store.pending(source_id="source", destination_id="local"), newer
            )

    def test_unsupported_database_is_rejected_without_overwriting_its_schema(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "other.sqlite"
            with sqlite3.connect(path) as connection:
                connection.execute("CREATE TABLE important(value TEXT)")
                connection.execute("INSERT INTO important VALUES('keep me')")
            before = path.read_bytes()
            with self.assertRaises(StateError):
                StateStore(path)
            self.assertEqual(path.read_bytes(), before)
            with sqlite3.connect(path) as connection:
                self.assertEqual(
                    connection.execute("SELECT value FROM important").fetchone()[0],
                    "keep me",
                )

    def test_nonfinite_cursor_rolls_back_preparation(self) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            StateStore(Path(directory) / "state.sqlite") as store,
        ):
            with self.assertRaises(StateError):
                store.prepare(
                    changes(),
                    source_id="source",
                    destination_id="local",
                    next_cursor={"offset": float("nan")},
                )
            self.assertIsNone(store.pending(source_id="source", destination_id="local"))
            self.assertEqual(
                store.checkpoint(source_id="source", destination_id="local").revision, 0
            )

    def test_receipt_revisions_reject_unsigned_overflow_and_bool(self) -> None:
        base = {
            "schema_version": 2,
            "source_id": "source",
            "batch_id": "batch",
            "source_revision": 1,
            "silver_revision": 1,
            "gold_ready": False,
        }
        for invalid in (True, -1, 1 << 64):
            value = {**base, "source_revision": invalid}
            raw = (
                orjson.dumps(base).replace(
                    b'"source_revision":1', b'"source_revision":18446744073709551616'
                )
                if invalid == 1 << 64
                else orjson.dumps(value)
            )
            with self.assertRaises(InvalidReceipt):
                decode_receipt(raw)
