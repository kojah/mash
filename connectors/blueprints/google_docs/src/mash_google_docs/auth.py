"""OAuth as the user through a Desktop client; the refresh token stays in a 0600 file.

Only `drive.readonly` is requested. Narrower scopes cannot do the job:
`drive.metadata.readonly` cannot export text or read comments, `drive.file`
only sees files this app created or the user picked, so folders cannot be
traversed, and `documents.readonly` cannot list folders, drives or comments.
"""

import base64
import hashlib
import os
import secrets
import time
import urllib.parse
import urllib.request
from collections.abc import Callable
from pathlib import Path

import orjson
from pydantic import BaseModel, ConfigDict, SecretStr

from .api import HttpError, Send, Sleep, fetch

SCOPE = "https://www.googleapis.com/auth/drive.readonly"
AUTH_URI = "https://accounts.google.com/o/oauth2/v2/auth"
TOKEN_URI = "https://oauth2.googleapis.com/token"
# Desktop clients accept any loopback port. Nothing needs to listen there: the
# browser's failed page still shows the redirect URL, which the user pastes
# back, so consent works when the connector runs over SSH.
REDIRECT_URI = "http://127.0.0.1:8085/"
# Refresh a minute early so a token never expires between check and request.
EXPIRY_MARGIN_SECONDS = 60


class AuthorizationRequired(Exception):
    """The stored authorization is missing, expired or revoked; rerun `auth`."""


class Client(BaseModel):
    model_config = ConfigDict(extra="ignore", frozen=True)
    client_id: str
    client_secret: SecretStr


class ClientFile(BaseModel):
    """The JSON Google Cloud Console downloads for a Desktop OAuth client."""

    model_config = ConfigDict(extra="ignore", frozen=True)
    installed: Client


class StoredToken(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    refresh_token: SecretStr
    scope: str


class TokenResponse(BaseModel):
    model_config = ConfigDict(extra="ignore", frozen=True)
    access_token: SecretStr
    expires_in: int
    refresh_token: SecretStr | None = None
    scope: str | None = None


def load_client(path: Path) -> Client:
    try:
        return ClientFile.model_validate_json(path.read_bytes()).installed
    except ValueError:
        raise ValueError(
            "client_secret must be the JSON of a Desktop app OAuth client"
        ) from None


def consent_url(client: Client, *, state: str, verifier: str) -> str:
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest())
    query = {
        "client_id": client.client_id,
        "redirect_uri": REDIRECT_URI,
        "response_type": "code",
        "scope": SCOPE,
        # Offline access with forced consent always returns a refresh token.
        "access_type": "offline",
        "prompt": "consent",
        "state": state,
        "code_challenge": challenge.rstrip(b"=").decode(),
        "code_challenge_method": "S256",
    }
    return f"{AUTH_URI}?{urllib.parse.urlencode(query)}"


def authorization_code(pasted: str, state: str) -> str:
    """Accept the full redirect URL (state-checked) or the bare code."""
    pasted = pasted.strip()
    if not pasted.startswith("http"):
        if not pasted:
            raise ValueError("no authorization code was pasted")
        return pasted
    query = urllib.parse.parse_qs(urllib.parse.urlsplit(pasted).query)
    if query.get("state") != [state]:
        raise ValueError("the pasted redirect belongs to a different consent request")
    if "error" in query or "code" not in query:
        raise ValueError("consent was not granted")
    return query["code"][0]


def authorize(
    client: Client,
    token_path: Path,
    ask: Callable[[str], str],
    *,
    send: Send,
    sleep: Sleep = time.sleep,
) -> None:
    """Run one consent: `ask` shows the URL and returns what the user pasted."""
    state = secrets.token_urlsafe(16)
    verifier = secrets.token_urlsafe(64)
    code = authorization_code(
        ask(consent_url(client, state=state, verifier=verifier)), state
    )
    form = {
        "code": code,
        "client_id": client.client_id,
        "client_secret": client.client_secret.get_secret_value(),
        "redirect_uri": REDIRECT_URI,
        "grant_type": "authorization_code",
        "code_verifier": verifier,
    }
    response = TokenResponse.model_validate_json(post(send, sleep, form))
    if response.refresh_token is None:
        raise ValueError("Google returned no refresh token; run auth again")
    if SCOPE not in (response.scope or "").split():
        raise ValueError("read-only Drive access was not granted")
    save_token(
        token_path, StoredToken(refresh_token=response.refresh_token, scope=SCOPE)
    )


def post(send: Send, sleep: Sleep, form: dict[str, str]) -> bytes:
    request = urllib.request.Request(
        TOKEN_URI,
        data=urllib.parse.urlencode(form).encode(),
        headers={"Content-Type": "application/x-www-form-urlencoded"},
        method="POST",
    )
    return fetch(send, sleep, request)


def save_token(path: Path, token: StoredToken) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.unlink(missing_ok=True)
    # Created 0600 so the secret is never briefly readable by others.
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(
            orjson.dumps(
                {
                    "refresh_token": token.refresh_token.get_secret_value(),
                    "scope": token.scope,
                }
            )
        )
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def load_token(path: Path) -> StoredToken:
    try:
        mode = path.stat().st_mode
    except FileNotFoundError:
        raise AuthorizationRequired("no stored Google authorization") from None
    if mode & 0o077:
        # Re-running auth replaces it with a private 0600 file.
        raise AuthorizationRequired(f"{path} is accessible to other users")
    return StoredToken.model_validate_json(path.read_bytes())


class Credentials:
    """Short-lived access tokens from the stored refresh token, kept in memory."""

    def __init__(
        self,
        client: Client,
        token: StoredToken,
        *,
        send: Send,
        sleep: Sleep = time.sleep,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self.client = client
        self.token = token
        self.send = send
        self.sleep = sleep
        self.clock = clock
        self.access: SecretStr | None = None
        self.expires = 0.0

    def header(self) -> str:
        if self.access is None or self.clock() >= self.expires:
            self.refresh()
        assert self.access is not None
        return f"Bearer {self.access.get_secret_value()}"

    def refresh(self) -> None:
        form = {
            "client_id": self.client.client_id,
            "client_secret": self.client.client_secret.get_secret_value(),
            "refresh_token": self.token.refresh_token.get_secret_value(),
            "grant_type": "refresh_token",
        }
        try:
            body = post(self.send, self.sleep, form)
        except HttpError as error:
            # Expired (an External app left in Testing lasts 7 days), revoked or
            # unused for six months: only a new consent can recover.
            if error.reason == "invalid_grant":
                raise AuthorizationRequired(
                    "Google rejected the stored authorization (expired or revoked)"
                ) from None
            raise
        response = TokenResponse.model_validate_json(body)
        self.access = response.access_token
        self.expires = self.clock() + response.expires_in - EXPIRY_MARGIN_SECONDS
