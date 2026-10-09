"""The actual Rust silver transaction supplies SDK receipts across retry/restart."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from mash_sdk import Changes
from mash_sdk._state import StateStore
from tests.objects import channel, message, note


class RustIngestionTests(unittest.TestCase):
    def test_lost_response_replays_same_batch_and_advances_sdk_only_on_receipt(
        self,
    ) -> None:
        binary = os.environ.get("MASH_CONNECTOR_CONTRACT_BINARY")
        if binary is None:
            self.skipTest("set MASH_CONNECTOR_CONTRACT_BINARY for real Rust ingestion")
        if not Path(binary).is_file():
            self.fail("configured Rust ingestion binary is missing")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            state = root / "sdk.sqlite"
            silver = root / "silver.sqlite"
            records = Changes.upsert(
                channel(id="channel", title="Engineering"),
                message(id="message", parent="channel", content="A participant's post"),
                note(id="note", content="A standalone note"),
            )
            with StateStore(state) as store:
                pending = store.prepare(
                    records, source_id="source", destination_id="local"
                )
            command = [binary, "apply", "--database", str(silver)]
            accepted = subprocess.run(
                command,
                input=pending.payload,
                capture_output=True,
                check=False,
                timeout=30,
            )
            self.assertEqual(accepted.returncode, 0, accepted.stderr.decode())
            # Simulate a committed server response never reaching the SDK.
            with StateStore(state) as reopened:
                recovered = reopened.pending(source_id="source", destination_id="local")
                self.assertEqual(recovered, pending)
                self.assertEqual(
                    reopened.checkpoint(
                        source_id="source", destination_id="local"
                    ).revision,
                    0,
                )
                replayed = subprocess.run(
                    command,
                    input=pending.payload,
                    capture_output=True,
                    check=False,
                    timeout=30,
                )
                self.assertEqual(replayed.returncode, 0, replayed.stderr.decode())
                self.assertEqual(replayed.stdout, accepted.stdout)
                receipt = reopened.acknowledge(
                    replayed.stdout, source_id="source", destination_id="local"
                )
                self.assertEqual(receipt.source_revision, 1)
                self.assertFalse(receipt.gold_ready)
                self.assertIsNone(
                    reopened.pending(source_id="source", destination_id="local")
                )
