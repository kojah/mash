"""Capture discovery and continuation guards through the actual MCP transport."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
import unittest.mock
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
            'identity': 'https://example.test/repo'},
            'source_fingerprint': os.environ.get('CAPTURE_FINGERPRINT', 'a')*64}
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
            "cwd": str(self.root),
            "last_assistant_message": "Done.",
        }

    def turn(self, name: str) -> str | None:
        return record_capture.context({**self.payload, "turn_id": name})

    def clock(self) -> unittest.mock.MagicMock:
        clock = patch.object(record_capture.time, "time", return_value=1000.0)
        self.addCleanup(clock.stop)
        return clock.start()

    def test_first_interval_is_quiet_then_one_check_per_interval(self) -> None:
        now = self.clock()
        self.assertIsNone(self.turn("first"))
        now.return_value += record_capture.MIN_INTERVAL - 1
        self.assertIsNone(self.turn("too soon"))
        self.assertFalse(self.log.exists(), "a quiet turn never starts Mash")
        now.return_value += 1
        text = self.turn("due") or ""
        for expected in (
            '"session_id":"host-session"',
            '{"kind":"local_project","path":',
            "find_record_matches",
            "user's own explicit endorsement",
            "never review older chats",
            "do not mention this check",
        ):
            self.assertIn(expected, text)
        self.assertLess(len(text), 2500)
        self.assertNotIn("older-chat", text)
        self.assertNotIn("extract_records", text)
        self.assertEqual(
            json.loads(self.log.read_text()), {"session_id": "host-session"}
        )
        self.assertIsNone(self.turn("due"), "the same turn is asked once")
        self.assertIsNone(
            record_capture.context(
                {**self.payload, "turn_id": "reentry", "stop_hook_active": True}
            )
        )
        now.return_value += record_capture.MIN_INTERVAL - 1
        self.assertIsNone(self.turn("still within the interval"))
        now.return_value += 1
        self.assertIsNotNone(self.turn("next interval"))
        now.return_value -= 10 * record_capture.MIN_INTERVAL
        self.assertIsNone(self.turn("clock moved backwards restarts the interval"))
        now.return_value += record_capture.MIN_INTERVAL
        self.assertIsNotNone(self.turn("and resumes"))

    def test_legacy_host_response_fingerprint_and_explicit_opt_out(self) -> None:
        now = self.clock()
        self.payload.pop("turn_id")
        self.assertIsNone(record_capture.context(self.payload))
        now.return_value += record_capture.MIN_INTERVAL
        self.payload["last_assistant_message"] = "Later."
        self.assertIsNotNone(record_capture.context(self.payload))
        self.assertIsNone(record_capture.context(self.payload))
        now.return_value += record_capture.MIN_INTERVAL
        os.environ["MASH_RECORD_CAPTURE"] = "off"
        self.assertIsNone(
            record_capture.context(
                {**self.payload, "last_assistant_message": "Another"}
            )
        )

    def test_disabled_policy_is_quiet_and_keeps_the_interval_open(self) -> None:
        now = self.clock()
        self.assertIsNone(self.turn("first"))
        now.return_value += record_capture.MIN_INTERVAL
        os.environ["CAPTURE_MODE"] = "disabled"
        self.assertIsNone(self.turn("disabled"))
        os.environ["CAPTURE_MODE"] = "empty"
        self.assertIsNotNone(self.turn("an unindexed session can still save"))

    def test_entrypoint_fails_open_for_bad_transport_data_and_timeout(self) -> None:
        hook = Path(record_capture.__file__)
        state = self.root / "state"
        state.mkdir()
        key = record_capture.hashlib.sha256(b"host-session").hexdigest()
        (state / f"capture-{key}.json").write_text(
            json.dumps({"version": 1, "turn": "", "prompted_at": 1})
        )
        for mode in ("error", "sleep"):
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
