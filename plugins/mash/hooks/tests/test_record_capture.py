"""The occasional memory check: its timing and policy through the actual MCP
transport, and its delivery with a prompt instead of at the end of a turn."""

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
import prompt_recall  # noqa: E402
import record_capture  # noqa: E402

SERVER = """
import json, os, sys, time
if sys.argv[1:] == ['--version']:
    old = os.environ.get('CAPTURE_MODE') == 'old-binary'
    cites = ['message_id'] + ([] if old else ['session_id'])
    print(json.dumps({'record_citations': cites}))
    sys.exit(0)
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
            "hook_event_name": "UserPromptSubmit",
            "session_id": "host-session",
            "cwd": str(self.root),
            "prompt": "add a unit test for the parser",
        }

    def prompt(self, **change: object) -> str | None:
        return record_capture.context({**self.payload, **change})

    def clock(self) -> unittest.mock.MagicMock:
        clock = patch.object(record_capture.time, "time", return_value=1000.0)
        self.addCleanup(clock.stop)
        return clock.start()

    def test_first_interval_is_quiet_then_one_check_per_interval(self) -> None:
        now = self.clock()
        self.assertIsNone(self.prompt())
        now.return_value += record_capture.MIN_INTERVAL - 1
        self.assertIsNone(self.prompt())
        self.assertFalse(self.log.exists(), "a quiet prompt never starts Mash")
        now.return_value += 1
        text = self.prompt() or ""
        for expected in (
            '"session_id":"host-session"',
            '{"kind":"local_project","path":',
            "find_record_matches",
            "user's own explicit endorsement",
            "never review older chats",
            "while you handle this prompt",
            "do not mention this check",
        ):
            self.assertIn(expected, text)
        self.assertLess(len(text), 2500)
        self.assertNotIn("older-chat", text)
        self.assertNotIn("extract_records", text)
        self.assertEqual(
            json.loads(self.log.read_text()), {"session_id": "host-session"}
        )
        self.assertIsNone(self.prompt(), "the next prompt is left alone")
        now.return_value += record_capture.MIN_INTERVAL - 1
        self.assertIsNone(self.prompt())
        now.return_value += 1
        self.assertIsNotNone(self.prompt())
        now.return_value -= 10 * record_capture.MIN_INTERVAL
        self.assertIsNone(self.prompt(), "a clock moved backwards restarts it")
        now.return_value += record_capture.MIN_INTERVAL
        self.assertIsNotNone(self.prompt())

    def test_slash_commands_other_sessions_and_opt_out(self) -> None:
        now = self.clock()
        self.assertIsNone(self.prompt())
        now.return_value += record_capture.MIN_INTERVAL
        # A slash command carries its own instructions; the check waits.
        self.assertIsNone(self.prompt(prompt="/compact"))
        # Another session keeps its own interval.
        self.assertIsNone(self.prompt(session_id="other-session"))
        os.environ["MASH_RECORD_CAPTURE"] = "off"
        self.assertIsNone(self.prompt())
        os.environ["MASH_RECORD_CAPTURE"] = "on"
        self.assertIsNotNone(self.prompt())
        for payload in (
            {},
            [],
            {**self.payload, "prompt": 5},
            {**self.payload, "cwd": 1},
        ):
            self.assertIsNone(record_capture.context(payload))

    def test_disabled_policy_is_quiet_and_keeps_the_interval_open(self) -> None:
        now = self.clock()
        self.assertIsNone(self.prompt())
        now.return_value += record_capture.MIN_INTERVAL
        os.environ["CAPTURE_MODE"] = "disabled"
        self.assertIsNone(self.prompt())
        os.environ["CAPTURE_MODE"] = "old-binary"
        self.assertIsNone(self.prompt(), "a binary without session citations")
        os.environ["CAPTURE_MODE"] = "empty"
        self.assertIsNotNone(self.prompt(), "an unindexed session can still save")

    def due(self) -> None:
        """Leaves the session with a check due on its next prompt."""
        state = self.root / "state"
        state.mkdir(exist_ok=True)
        key = record_capture.hashlib.sha256(b"host-session").hexdigest()
        (state / f"capture-{key}.json").write_text(
            json.dumps({"version": 1, "prompted_at": 1})
        )

    def test_check_rides_with_the_prompt_and_never_holds_a_turn_open(self) -> None:
        hook = Path(prompt_recall.__file__)
        self.due()
        result = subprocess.run(
            [sys.executable, str(hook)],
            input=json.dumps(self.payload).encode(),
            capture_output=True,
            timeout=6,
        )
        output = json.loads(result.stdout)
        # Context beside the prompt: no decision that would block or continue.
        self.assertEqual(list(output), ["hookSpecificOutput"])
        self.assertEqual(
            output["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit"
        )
        self.assertIn(
            "Mash memory check", output["hookSpecificOutput"]["additionalContext"]
        )
        hooks = json.loads((hook.parent / "hooks.json").read_text())["hooks"]
        self.assertNotIn("Stop", hooks)

    def test_check_joins_other_nudges_and_covers_a_lasting_prompt_once(self) -> None:
        self.due()
        lasting = "from now on always use tabs. what did we do yesterday"
        text = prompt_recall.context({**self.payload, "prompt": lasting}) or ""
        self.assertIn("search Mash history", text)
        self.assertIn("This prompt may itself state", text)
        self.assertEqual(text.count("use the Mash MCP record tool"), 1)
        # With the nudges switched off the check still arrives, alone.
        self.due()
        with patch.dict(os.environ, {"MASH_PROMPT_RECALL": "off"}):
            alone = prompt_recall.context({**self.payload, "prompt": lasting}) or ""
        self.assertTrue(alone.startswith("Mash memory check"))
        self.assertNotIn("This prompt may itself state", alone)

    def test_prompt_hook_fails_open_for_bad_transport_data_and_timeout(self) -> None:
        hook = Path(prompt_recall.__file__)
        for mode in ("error", "sleep"):
            self.due()
            os.environ["CAPTURE_MODE"] = mode
            started = time.monotonic()
            result = subprocess.run(
                [sys.executable, str(hook)],
                input=json.dumps(self.payload).encode(),
                capture_output=True,
                timeout=6,
            )
            self.assertEqual((result.returncode, result.stdout), (0, b""), mode)
            self.assertLess(time.monotonic() - started, 4)


if __name__ == "__main__":
    unittest.main()
