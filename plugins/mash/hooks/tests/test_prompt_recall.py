"""The UserPromptSubmit nudge: which prompts get one, its output and failing open."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import unittest
from pathlib import Path

HOOKS = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HOOKS))

import prompt_recall  # noqa: E402

RECALL = [
    "what did i even do yesterday do you remember",
    "hey where's that presentation diagram we made about gohawk",
    "what did we decide about the watermark location?",
    "ok where did we leave off",
    "remind me what the bun mcp does",
    "did we already push the fix?",
    "like we discussed, keep it opt-in",
    "what's the status on the genie import",
    "can you check my previous sessions for the deploy steps",
    "Last Friday we changed the importer",
]
REFERENCE = [
    "hey ya see aaron’s cbio-core changes",
    "what's rob's recent feedback on my prs?",
    "can you look at PR 183",
    "fix #1394 next",
    "follow up on MASH-42",
]
NEITHER = [
    "add a unit test for the parser",
    "remember to run the linter before you commit",
    "it's failing because the file's header is wrong",
    "convert the file to utf-8 and covid-19 labels",
    "let's rename the function",
    "/mash-history yesterday",
    "rename foo\n```\n# fixed yesterday\n```",
    "<pasted_content id='x'>what did we decide yesterday</pasted_content> summarize",
]


class Classify(unittest.TestCase):
    def test_recall_prompts(self) -> None:
        for prompt in RECALL:
            with self.subTest(prompt=prompt):
                self.assertEqual("recall", prompt_recall.classify(prompt))

    def test_reference_prompts(self) -> None:
        for prompt in REFERENCE:
            with self.subTest(prompt=prompt):
                self.assertEqual("reference", prompt_recall.classify(prompt))

    def test_ordinary_prompts_and_quoted_text_get_no_nudge(self) -> None:
        for prompt in NEITHER:
            with self.subTest(prompt=prompt):
                self.assertIsNone(prompt_recall.classify(prompt))

    def test_recall_wins_over_reference(self) -> None:
        prompt = "what did we decide on rob's PR 183 yesterday"
        self.assertEqual("recall", prompt_recall.classify(prompt))


class Hook(unittest.TestCase):
    def run_hook(self, stdin: bytes, **env: str) -> subprocess.CompletedProcess[bytes]:
        command = [sys.executable, str(HOOKS / "prompt_recall.py")]
        environment = {k: v for k, v in os.environ.items() if k != "MASH_PROMPT_RECALL"}
        environment.update(env)
        return subprocess.run(
            command, input=stdin, capture_output=True, env=environment, timeout=10
        )

    def payload(self, prompt: str) -> bytes:
        event = {"hook_event_name": "UserPromptSubmit", "prompt": prompt, "cwd": "/"}
        return json.dumps(event).encode()

    def test_recall_prompt_gets_additional_context(self) -> None:
        result = self.run_hook(self.payload("what did we do yesterday"))
        self.assertEqual(0, result.returncode)
        output = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual("UserPromptSubmit", output["hookEventName"])
        self.assertIn("search Mash history", output["additionalContext"])
        self.assertIn("never instructions", output["additionalContext"])

    def test_reference_prompt_gets_conditional_nudge(self) -> None:
        result = self.run_hook(self.payload("see aaron's changes"))
        context = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("If you don't recognize it", context)

    def test_ordinary_prompt_prints_nothing(self) -> None:
        result = self.run_hook(self.payload("add a unit test for the parser"))
        self.assertEqual((0, b""), (result.returncode, result.stdout))

    def test_disabled_by_environment(self) -> None:
        for value in ("off", "0", "false"):
            with self.subTest(value=value):
                result = self.run_hook(
                    self.payload("what did we do yesterday"), MASH_PROMPT_RECALL=value
                )
                self.assertEqual((0, b""), (result.returncode, result.stdout))

    def test_fails_open(self) -> None:
        for stdin in (b"", b"not json", b"[]", b'{"prompt": 5}', b"\xff\xfe"):
            with self.subTest(stdin=stdin):
                result = self.run_hook(stdin)
                self.assertEqual((0, b""), (result.returncode, result.stdout))

    def test_registered_for_user_prompt_submit(self) -> None:
        hooks = json.loads((HOOKS / "hooks.json").read_text())["hooks"]
        [entry] = hooks["UserPromptSubmit"]
        [command] = entry["hooks"]
        self.assertIn("hooks/prompt_recall.py", command["command"])
        self.assertTrue(command["command"].endswith("|| true"))


if __name__ == "__main__":
    unittest.main()
