"""Exercise the real deployment decision with an isolated GitHub API substitute."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class DeploymentGateTests(unittest.TestCase):
    revision = "a" * 40
    script = Path(__file__).with_name("deploy-gate")

    def run_gate(
        self,
        *,
        event="workflow_dispatch",
        ref="refs/heads/main",
        sha=None,
        head=None,
        api_exit=0,
        requested=True,
    ):
        with tempfile.TemporaryDirectory(prefix="deployment-gate-") as directory:
            root = Path(directory)
            api = root / "gh"
            api.write_text(
                "#!/bin/sh\n"
                'printf "%s\\n" "$*" > "$API_CALLS"\n'
                'printf "%s\\n" "$API_HEAD"\n'
                'exit "$API_EXIT"\n'
            )
            api.chmod(0o700)
            output = root / "output"
            calls = root / "calls"
            environment = {
                **os.environ,
                "PATH": f"{root}:{os.environ['PATH']}",
                "GITHUB_REF": ref,
                "GITHUB_EVENT_NAME": event,
                "GITHUB_SHA": sha if sha is not None else self.revision,
                "GITHUB_REPOSITORY": "example/mash",
                "GITHUB_OUTPUT": str(output),
                "API_HEAD": head if head is not None else self.revision,
                "API_EXIT": str(api_exit),
                "API_CALLS": str(calls),
            }
            if requested is None:
                environment.pop("MASH_DEPLOY_REQUESTED", None)
            else:
                environment["MASH_DEPLOY_REQUESTED"] = str(requested).lower()
            result = subprocess.run(
                ["bash", str(self.script)],
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
            return (
                result.returncode,
                output.read_text() if output.exists() else "",
                calls.read_text() if calls.exists() else "",
            )

    def test_current_validated_main(self):
        code, output, calls = self.run_gate()
        self.assertEqual((code, output), (0, "deploy=true\n"))
        self.assertEqual(
            calls, "api repos/example/mash/git/ref/heads/main --jq .object.sha\n"
        )

    def test_manual_run_requires_explicit_release_request(self):
        for requested in (False, None):
            with self.subTest(requested=requested):
                self.assertEqual(
                    self.run_gate(requested=requested), (0, "deploy=false\n", "")
                )

    def test_superseded_run_cannot_deploy(self):
        code, output, _ = self.run_gate(head="b" * 40)
        self.assertEqual((code, output), (0, "deploy=false\n"))

    def test_other_events_and_branches_do_not_contact_api(self):
        for event, ref in (
            ("push", "refs/heads/main"),
            ("pull_request", "refs/heads/main"),
            ("workflow_run", "refs/heads/main"),
            ("workflow_dispatch", "refs/heads/feature"),
            ("push", "refs/tags/v1"),
        ):
            with self.subTest(event=event, ref=ref):
                self.assertEqual(
                    self.run_gate(event=event, ref=ref), (0, "deploy=false\n", "")
                )

    def test_invalid_revisions_and_api_failure_fail_closed(self):
        for parameters in (
            {"sha": ""},
            {"sha": "main"},
            {"head": ""},
            {"head": "null"},
            {"api_exit": 1},
        ):
            with self.subTest(parameters=parameters):
                code, output, _ = self.run_gate(**parameters)
                self.assertNotEqual(code, 0)
                self.assertEqual(output, "")


if __name__ == "__main__":
    unittest.main()
