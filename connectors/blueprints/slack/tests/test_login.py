import socket
import stat
import tempfile
import threading
import unittest
import urllib.parse
import urllib.request
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_slack import TEAM, TOKEN, FakeSlack

from mash_slack import login
from mash_slack.cli import main
from mash_slack.config import load


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


class Browser:
    """Stands in for the person approving Mash in Slack's page."""

    def __init__(self, slack: FakeSlack, state: str | None = None) -> None:
        self.slack = slack
        self.state = state
        self.opened: list[str] = []

    def __call__(self, url: str) -> None:
        self.opened.append(url)
        query = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(url).query))
        self.slack.codes["code-1"] = query["code_challenge"]
        reply = urllib.parse.urlencode(
            {"code": "code-1", "state": self.state or query["state"]}
        )
        threading.Thread(
            target=urllib.request.urlopen,
            args=(f"{query['redirect_uri']}?{reply}",),
            daemon=True,
        ).start()


class LoginTests(unittest.TestCase):
    def test_sign_in_creates_a_private_token_and_a_working_configuration(self) -> None:
        port = free_port()
        with tempfile.TemporaryDirectory() as directory, FakeSlack() as slack:
            config = Path(directory) / "slack.toml"
            browser = Browser(slack)
            outcome = login.run(config, "123.456", port, browser, 10, slack.base_url)
            settings = load(config)
            mode = stat.S_IMODE(outcome.token_file.stat().st_mode)
            saved = outcome.token_file.read_text().strip()
        query = dict(
            urllib.parse.parse_qsl(urllib.parse.urlsplit(browser.opened[0]).query)
        )
        self.assertEqual(query["code_challenge_method"], "S256")
        self.assertEqual(
            query["redirect_uri"], f"http://localhost:{port}/slack/callback"
        )
        self.assertIn("channels:history", query["user_scope"])
        self.assertNotIn("scope", query)  # Desktop sign-in cannot ask for bot scopes.
        self.assertTrue(outcome.created_config)
        self.assertEqual((settings.team_id, settings.principal), (TEAM, "U0ALICE"))
        self.assertEqual(settings.token_file, outcome.token_file)
        self.assertEqual(saved, TOKEN)
        self.assertEqual(mode, 0o600)
        self.assertEqual(settings.channels, ())

    def test_an_existing_configuration_keeps_its_account(self) -> None:
        with tempfile.TemporaryDirectory() as directory, FakeSlack() as slack:
            config = Path(directory) / "slack.toml"
            config.write_text(
                f"team_id='{TEAM}'\nprincipal='U0BOB'\ntoken_file='bob.token'\n"
                "state_dir='state'\nchannels=['C01']\n"
                "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
            )
            with self.assertRaises(login.LoginFailed) as raised:
                login.run(
                    config, "123.456", free_port(), Browser(slack), 10, slack.base_url
                )
            self.assertFalse((Path(directory) / "bob.token").exists())
        self.assertIn("U0BOB", str(raised.exception))

    def test_a_reply_from_another_sign_in_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory, FakeSlack() as slack:
            config = Path(directory) / "slack.toml"
            with self.assertRaises(login.LoginFailed):
                login.run(
                    config,
                    "123.456",
                    free_port(),
                    Browser(slack, "forged"),
                    10,
                    slack.base_url,
                )
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_the_app_manifest_reads_only_and_signs_in_without_a_secret(self) -> None:
        manifest = login.manifest(4567)
        self.assertTrue(manifest["oauth_config"]["pkce_enabled"])
        self.assertFalse(manifest["settings"]["token_rotation_enabled"])
        self.assertNotIn("bot", manifest["oauth_config"]["scopes"])
        self.assertTrue(
            all(scope.endswith((":read", ":history")) for scope in login.SCOPES)
        )
        link = urllib.parse.urlsplit(login.create_app_url(4567))
        encoded = dict(urllib.parse.parse_qsl(link.query))["manifest_json"]
        self.assertEqual(orjson.loads(encoded), manifest)

    def test_the_command_reports_the_account_but_never_the_token(self) -> None:
        outcome = login.Outcome(
            login.Login(TEAM, "U0ALICE", TOKEN), Path("/x/slack.token"), True
        )
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "new.toml"
            with patch("mash_slack.login.run", return_value=outcome):
                result = CliRunner().invoke(
                    main,
                    [
                        "--config",
                        str(config),
                        "login",
                        "--client-id",
                        "1.2",
                        "--no-browser",
                    ],
                )
        self.assertEqual(result.exit_code, 0, result.output)
        self.assertNotIn(TOKEN, result.output)
        report = orjson.loads(result.stdout)
        self.assertEqual((report["team_id"], report["principal"]), (TEAM, "U0ALICE"))
        self.assertIn("channels", report["next"][0])


if __name__ == "__main__":
    unittest.main()
