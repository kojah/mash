"""Capture discovery and continuation guards through the actual MCP transport."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import record_capture  # noqa: E402

SERVER = """
import json, os, sys, time
for line in sys.stdin:
    request = json.loads(line)
    if request['method'] == 'notifications/initialized':
        continue
    if request['method'] == 'initialize':
        result = {'protocolVersion': request['params']['protocolVersion']}
    else:
        assert request['params']['name'] == 'get_record_capture'
        with open(os.environ['CAPTURE_LOG'], 'w') as log:
            json.dump(request['params']['arguments'], log)
        mode = os.environ.get('CAPTURE_MODE', 'ok')
        if mode == 'sleep': time.sleep(20)
        task = {'chat_id': 'current-chat', 'scope': {'kind': 'repository',
            'identity': 'https://example.test/repo'}, 'source_fingerprint': 'a'*64}
        packet = {'enabled': mode != 'disabled', 'current': task,
            'backfill': {**task, 'chat_id': 'older-chat'},
            'backfill_since': 1000, 'next_offset': 7}
        if mode == 'empty': packet.update(current=None, backfill=None)
        if mode == 'invalid': task['source_fingerprint'] = 'untrusted'
        result = {'structuredContent': packet, 'isError': mode == 'error'}
    response = {'jsonrpc': '2.0', 'id': request['id'], 'result': result}
    print(json.dumps(response), flush=True)
"""


class RecordCapture(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.binary = self.root / "mash"
        self.binary.write_text(f"#!{sys.executable}\n" + SERVER)
        self.binary.chmod(0o755)
        self.log = self.root / "arguments.json"
        environment = patch.dict(
            os.environ,
            {
                "MASH_BIN": str(self.binary),
                "MASH_RECORD_CAPTURE": "on",
                "MASH_AUTO_INDEX": "off",
                "CAPTURE_MODE": "ok",
                "CAPTURE_LOG": str(self.log),
                "MASH_RECALL_STATE_DIR": str(self.root / "state"),
            },
        )
        environment.start()
        self.addCleanup(environment.stop)
        self.payload = {
            "session_id": "host-session",
            "turn_id": "turn-one",
            "stop_hook_active": False,
            "last_assistant_message": "Done.",
        }

    def test_capture_requests_review_then_suppresses_same_turn_and_reentry(
        self,
    ) -> None:
        text = record_capture.context(self.payload)
        self.assertIsNotNone(text)
        assert text is not None
        for expected in (
            "exact quotes",
            "find_record_matches",
            "user's own explicit endorsement",
            "current-chat",
            "Work only on new outcomes",
        ):
            self.assertIn(expected, text)
        self.assertLess(len(text), 5000)
        self.assertNotIn("older-chat", text)
        self.assertNotIn("extract_records", text)
        self.assertIsNone(record_capture.context(self.payload))
        self.assertIsNone(
            record_capture.context({**self.payload, "stop_hook_active": True})
        )
        self.assertIsNotNone(
            record_capture.context({**self.payload, "turn_id": "turn-two"})
        )
        self.assertEqual(
            json.loads(self.log.read_text()), {"session_id": "host-session"}
        )

    def test_legacy_host_response_fingerprint_and_explicit_opt_out(self) -> None:
        self.payload.pop("turn_id")
        self.assertIsNotNone(record_capture.context(self.payload))
        self.assertIsNone(record_capture.context(self.payload))
        os.environ["MASH_RECORD_CAPTURE"] = "off"
        self.assertIsNone(
            record_capture.context(
                {**self.payload, "last_assistant_message": "Another"}
            )
        )

    def test_disabled_policy_and_no_indexed_work_do_not_continue(self) -> None:
        os.environ["CAPTURE_MODE"] = "disabled"
        self.assertIsNone(record_capture.context(self.payload))
        os.environ["CAPTURE_MODE"] = "empty"
        self.assertIsNone(record_capture.context(self.payload))
        os.environ["MASH_AUTO_INDEX"] = "on"
        text = record_capture.context({**self.payload, "turn_id": "next"})
        self.assertIn("Do not rebuild or install models", text or "")

    def test_entrypoint_fails_open_for_bad_transport_data_and_timeout(self) -> None:
        hook = Path(record_capture.__file__)
        for mode in ("invalid", "error", "sleep"):
            os.environ["CAPTURE_MODE"] = mode
            started = time.monotonic()
            result = subprocess.run(
                [sys.executable, str(hook)],
                input=json.dumps(self.payload).encode(),
                capture_output=True,
                timeout=4,
            )
            self.assertEqual((result.returncode, result.stdout), (0, b""), mode)
            self.assertLess(time.monotonic() - started, 3)
        for payload in ({}, {**self.payload, "stop_hook_active": True}, []):
            self.assertIsNone(record_capture.context(payload))


if __name__ == "__main__":
    unittest.main()
