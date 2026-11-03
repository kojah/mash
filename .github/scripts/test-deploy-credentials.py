"""Verify the actual deployment preflight without contacting the fleet."""

import os
from pathlib import Path
import subprocess
import unittest


class DeploymentCredentialTests(unittest.TestCase):
    names = ("TS_OAUTH_CLIENT_ID", "TS_OAUTH_SECRET",
             "DEPLOY_SSH_PRIVATE_KEY", "DEPLOY_SSH_KNOWN_HOSTS", "DEPLOY_TARGETS")
    script = Path(__file__).with_name("deploy-credentials")
    sensitive = "private-credential-sentinel"

    def preflight(self, credentials):
        environment = {key: value for key, value in os.environ.items()
                       if key not in self.names}
        result = subprocess.run(["bash", str(self.script)],
                                env={**environment, **credentials},
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.stdout, "")
        self.assertNotIn(self.sensitive, result.stderr)
        return result

    def test_missing_configuration_reports_every_required_name(self):
        result = self.preflight({})
        self.assertNotEqual(result.returncode, 0)
        for name in self.names:
            self.assertIn(f"Missing deployment repository secret: {name}\n", result.stderr)
        self.assertIn("GitHub Actions", result.stderr)

    def test_each_unset_empty_or_whitespace_secret_fails(self):
        for name in self.names:
            for value in (None, "", " \t\n"):
                with self.subTest(name=name, value=value):
                    credentials = dict.fromkeys(self.names, self.sensitive)
                    if value is None:
                        del credentials[name]
                    else:
                        credentials[name] = value
                    result = self.preflight(credentials)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f"Missing deployment repository secret: {name}\n", result.stderr)
                    self.assertEqual(result.stderr.count("Missing deployment repository secret:"), 1)

    def test_complete_configuration_is_silent_including_multiline_values(self):
        credentials = dict.fromkeys(self.names, self.sensitive)
        credentials["DEPLOY_SSH_PRIVATE_KEY"] = f"\n{self.sensitive}\nsecond-key-line\n"
        result = self.preflight(credentials)
        self.assertEqual((result.returncode, result.stderr), (0, ""))

    def test_multiple_missing_names_do_not_expose_present_values(self):
        result = self.preflight({"TS_OAUTH_SECRET": self.sensitive})
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("Missing deployment repository secret: TS_OAUTH_SECRET", result.stderr)
        self.assertEqual(
            result.stderr.count("Missing deployment repository secret:"), len(self.names) - 1
        )


if __name__ == "__main__":
    unittest.main()
