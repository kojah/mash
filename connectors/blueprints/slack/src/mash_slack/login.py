"""`mash-slack login`: get a user token from Slack in the browser, without a secret.

Slack's desktop OAuth (PKCE, a localhost redirect, no client secret) issues a
user token for the app the person created from `manifest()`. The token is
written to a file only this user can read; it is never printed. Token rotation
stays off in the manifest, so the token lasts until revoked in Slack.
"""

import asyncio
import base64
import hashlib
import http.server
import os
import secrets
import threading
import time
import urllib.parse
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import orjson
from slack_sdk.web.async_client import AsyncWebClient

from .client import API
from .config import load

AUTHORIZE = "https://slack.com/oauth/v2/authorize"
PORT = 47643
CALLBACK = "/slack/callback"
# Read-only access to what the signing-in user can see. Desktop sign-in cannot
# request bot scopes, so the connector always reads as that user.
SCOPES = (
    "channels:read",
    "channels:history",
    "groups:read",
    "groups:history",
    "im:read",
    "im:history",
    "mpim:read",
    "mpim:history",
    "users:read",
)


class LoginFailed(Exception):
    """Slack refused, the browser never came back, or the reply did not match."""


@dataclass(frozen=True)
class Login:
    team_id: str
    principal: str
    token: str


def redirect(port: int) -> str:
    return f"http://localhost:{port}{CALLBACK}"


def manifest(port: int = PORT) -> dict:
    """An app made for this workspace only, so Slack keeps its normal limits."""
    return {
        "display_information": {
            "name": "Mash",
            "description": "Lets Mash index the Slack channels you choose, read-only.",
        },
        "oauth_config": {
            "redirect_urls": [redirect(port)],
            "scopes": {"user": list(SCOPES)},
            "pkce_enabled": True,
        },
        "settings": {"org_deploy_enabled": False, "token_rotation_enabled": False},
    }


def create_app_url(port: int = PORT) -> str:
    """Opens Slack's app creation with this manifest filled in."""
    encoded = urllib.parse.quote(orjson.dumps(manifest(port)).decode())
    return f"https://api.slack.com/apps?new_app=1&manifest_json={encoded}"


def challenge(verifier: str) -> str:
    digest = hashlib.sha256(verifier.encode()).digest()
    return base64.urlsafe_b64encode(digest).rstrip(b"=").decode()


def authorize_url(client_id: str, port: int, state: str, verifier: str) -> str:
    query = urllib.parse.urlencode(
        {
            "client_id": client_id,
            "user_scope": ",".join(SCOPES),
            "redirect_uri": redirect(port),
            "state": state,
            "code_challenge": challenge(verifier),
            "code_challenge_method": "S256",
        }
    )
    return f"{AUTHORIZE}?{query}"


def receive(port: int, state: str, start: Callable[[], None], timeout: float) -> str:
    """Serve one callback on localhost and return its authorization code."""
    reply: dict[str, str] = {}

    class Callback(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - the stdlib's handler name
            url = urllib.parse.urlsplit(self.path)
            query = dict(urllib.parse.parse_qsl(url.query))
            if url.path != CALLBACK:
                self.send_error(404)
                return
            reply.update(query)
            done = "code" in query and query.get("state") == state
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(
                b"Mash is signed in to Slack. You can close this tab."
                if done
                else b"Slack sign-in did not complete. Return to the terminal."
            )

        def log_message(self, format: str, *args: object) -> None:  # noqa: A002
            """The query carries the authorization code; keep it out of logs."""

    server = http.server.HTTPServer(("127.0.0.1", port), Callback)
    deadline = time.monotonic() + timeout
    with server:
        threading.Thread(target=start, daemon=True).start()
        while not reply:
            server.timeout = deadline - time.monotonic()
            if server.timeout <= 0:
                raise LoginFailed("timed out waiting for Slack in the browser")
            server.handle_request()
    if reply.get("state") != state:
        raise LoginFailed("the browser's reply did not come from this sign-in")
    if "error" in reply:
        raise LoginFailed(f"Slack did not authorize Mash: {reply['error']}")
    if "code" not in reply:
        raise LoginFailed("Slack's reply had no authorization code")
    return reply["code"]


async def exchange(
    client_id: str, code: str, verifier: str, port: int, base_url: str = API
) -> Login:
    """Trade the code for a user token; PKCE replaces the client secret."""
    slack = AsyncWebClient(base_url=base_url, timeout=60)
    response = await slack.api_call(
        "oauth.v2.access",
        data={
            "client_id": client_id,
            "code": code,
            "code_verifier": verifier,
            "redirect_uri": redirect(port),
        },
    )
    user = response.get("authed_user") or {}
    team = response.get("team") or {}
    if not (user.get("access_token") and user.get("id") and team.get("id")):
        raise LoginFailed("Slack's token response had no user token")
    return Login(team_id=team["id"], principal=user["id"], token=user["access_token"])


def save_token(path: Path, value: str) -> None:
    """Replace the file atomically; it is created readable by this user only."""
    path.parent.mkdir(parents=True, exist_ok=True)
    staged = path.with_name(f".{path.name}.{secrets.token_hex(4)}")
    descriptor = os.open(staged, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as file:
        file.write(value + "\n")
    os.replace(staged, path)


def configuration(login: Login, token_file: Path) -> str:
    """A starting configuration; channels stay empty until the person picks them."""
    source = f"{login.team_id}-{login.principal}"
    return (
        "# Written by mash-slack login. Paths are relative to this file; ~ expands.\n"
        f'team_id = "{login.team_id}"\n'
        f'principal = "{login.principal}" # The Slack user the token reads as.\n'
        f'token_file = "{token_file}"\n'
        f'state_dir = "~/.local/share/mash/connectors/slack/{source}"\n'
        "channels = [] # Add channel IDs (C...) to index; [] indexes nothing.\n"
        "# history_days = 90 # By message creation time; omit for all history.\n"
        "thread_days = 7\n"
        "poll_seconds = 300\n\n"
        "[[destinations]]\n"
        'type = "local"\n'
        'database = "~/.local/share/mash/mash.sqlite3"\n'
        'executable = "mash"\n'
    )


@dataclass(frozen=True)
class Outcome:
    login: Login
    token_file: Path
    created_config: bool


def run(
    config: Path,
    client_id: str,
    port: int,
    open_url: Callable[[str], None],
    timeout: float = 300,
    base_url: str = API,
) -> Outcome:
    """Sign in, then save the token for `config`, creating it if it is new.

    An existing configuration must belong to the same workspace and user, so
    a sign-in never redirects an existing source to someone else's history.
    """
    settings = load(config) if config.exists() else None
    verifier = secrets.token_urlsafe(64)
    state = secrets.token_urlsafe(24)
    url = authorize_url(client_id, port, state, verifier)
    code = receive(port, state, lambda: open_url(url), timeout)
    login = asyncio.run(exchange(client_id, code, verifier, port, base_url))
    if settings is not None:
        if (settings.team_id, settings.principal) != (login.team_id, login.principal):
            raise LoginFailed(
                f"{config} belongs to {settings.team_id}/{settings.principal}, but "
                f"you signed in as {login.team_id}/{login.principal}; use another "
                "configuration file for this account"
            )
        save_token(settings.token_file, login.token)
        return Outcome(login, settings.token_file, created_config=False)
    token_file = config.with_suffix(".token")
    save_token(token_file, login.token)
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(configuration(login, token_file))
    return Outcome(login, token_file, created_config=True)
