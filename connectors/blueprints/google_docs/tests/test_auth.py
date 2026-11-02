"""Consent, token storage, refresh failures, retries and secret hygiene, offline."""

import io
import logging
import os
import tempfile
import unittest
import urllib.parse
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_google import (
    ACCESS_TOKEN,
    CLIENT,
    CLIENT_SECRET,
    REFRESH_TOKEN,
    FakeGoogle,
    item,
    store_token,
)

from mash_google_docs.api import ATTEMPTS, HttpError
from mash_google_docs.auth import (
    SCOPE,
    AuthorizationRequired,
    authorize,
    load_token,
)
from mash_google_docs.cli import WrongPrincipal, main

SECRETS = (REFRESH_TOKEN, ACCESS_TOKEN, CLIENT_SECRET)


class AuthTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.google = FakeGoogle()

    def test_consent_accepts_pasted_redirect_and_stores_private_token(self) -> None:
        token = self.root / "config" / "mash" / "google-docs-token.json"
        shown: list[str] = []

        def ask(url: str) -> str:
            shown.append(url)
            query = urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)
            self.assertEqual(query["scope"], [SCOPE])
            self.assertEqual(query["code_challenge_method"], ["S256"])
            state = query["state"][0]
            return f"http://127.0.0.1:8085/?state={state}&code=4/abc&scope={SCOPE}"

        authorize(CLIENT, token, ask, send=self.google.send)
        self.assertEqual(token.stat().st_mode & 0o777, 0o600)
        self.assertEqual(
            load_token(token).refresh_token.get_secret_value(), REFRESH_TOKEN
        )
        self.assertNotIn(REFRESH_TOKEN, repr(load_token(token)))
        self.assertNotIn(CLIENT_SECRET, shown[0] + repr(CLIENT))

    def test_pasted_redirect_from_another_request_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            authorize(
                CLIENT,
                self.root / "token.json",
                lambda _: "http://127.0.0.1:8085/?state=forged&code=x",
                send=self.google.send,
            )

    def test_readable_token_file_is_refused(self) -> None:
        token = self.root / "token.json"
        store_token(token)
        os.chmod(token, 0o644)
        with self.assertRaises(AuthorizationRequired):
            load_token(token)

    def test_rate_limits_and_server_errors_retry_boundedly(self) -> None:
        self.google.add(item("a"))
        self.google.failures["files/a"] = [429, 503]
        drive = self.google.drive()
        self.assertEqual(drive.file("a").id, "a")
        self.google.failures["files/a"] = [429] * ATTEMPTS
        with self.assertRaises(HttpError) as raised:
            drive.file("a")
        self.assertEqual(raised.exception.status, 429)

    def test_expired_or_revoked_authorization_fails_with_typed_error(self) -> None:
        self.google.token_status = 400
        with self.assertRaises(AuthorizationRequired):
            self.google.drive().file("a")
        self.assertEqual(self.google.calls.count("/token"), 1, "no silent retries")

    def test_cli_reports_reauthorization_without_leaking_secrets(self) -> None:
        self.google.add(item("a", parent="top"))
        self.google.token_status = 400
        client = self.root / "client.json"
        client.write_bytes(
            orjson.dumps(
                {
                    "installed": {
                        "client_id": CLIENT.client_id,
                        "client_secret": CLIENT_SECRET,
                    }
                }
            )
        )
        store_token(self.root / "token.json")
        config = self.root / "google-docs.toml"
        config.write_text(
            "account_id='test'\nclient_secret='client.json'\ntoken='token.json'\n"
            "state_dir='state'\nfolders=['top']\n"
            "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
        )
        logs = io.StringIO()
        handler = logging.StreamHandler(logs)
        logging.getLogger().addHandler(handler)
        logging.getLogger().setLevel(logging.DEBUG)
        self.addCleanup(logging.getLogger().removeHandler, handler)
        patcher = patch("mash_google_docs.cli.urlopen", self.google.send)
        patcher.start()
        self.addCleanup(patcher.stop)
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            result = CliRunner().invoke(main, ["--config", str(config), "sync"])
        self.assertEqual(result.exit_code, 3)
        output = result.output + stdout.getvalue() + stderr.getvalue() + logs.getvalue()
        self.assertIn("authorization-required", output)
        self.assertIn("auth` again", output)
        for secret in SECRETS:
            self.assertNotIn(secret, output)


    def test_a_token_for_another_account_is_rejected(self) -> None:
        self.google.add(item("a", parent="top"))
        self.google.principal = "someone-else"
        client = self.root / "client.json"
        client.write_bytes(
            orjson.dumps(
                {
                    "installed": {
                        "client_id": CLIENT.client_id,
                        "client_secret": CLIENT_SECRET,
                    }
                }
            )
        )
        store_token(self.root / "token.json")
        config = self.root / "google-docs.toml"
        config.write_text(
            "account_id='test'\nclient_secret='client.json'\ntoken='token.json'\n"
            "state_dir='state'\nfolders=['top']\n"
            "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
        )
        patcher = patch("mash_google_docs.cli.urlopen", self.google.send)
        patcher.start()
        self.addCleanup(patcher.stop)
        result = CliRunner().invoke(main, ["--config", str(config), "sync"])
        self.assertIsInstance(result.exception, WrongPrincipal)


if __name__ == "__main__":
    unittest.main()
