"""An in-memory Google OAuth and Drive v3 endpoint behind the connector's `Send`."""

import re
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

import orjson

from mash_google_docs.auth import Client, Credentials, StoredToken, save_token
from mash_google_docs.drive import DOCUMENT, FOLDER, Drive

REFRESH_TOKEN = "1//refresh-secret-do-not-log"
ACCESS_TOKEN = "ya29.access-secret-do-not-log"
CLIENT_SECRET = "client-secret-do-not-log"
CLIENT = Client.model_validate(
    {"client_id": "client.apps.googleusercontent.com", "client_secret": CLIENT_SECRET}
)
PARENTS = re.compile(r"'([^']+)' in parents")


def item(
    id: str,
    *,
    parent: str | None = None,
    mime: str = DOCUMENT,
    created: str = "2026-10-01T00:00:00Z",
    version: str = "1",
    trashed: bool = False,
    drive: str | None = None,
) -> dict:
    return {
        "id": id,
        "name": f"Title {id}",
        "mimeType": mime,
        "createdTime": created,
        "modifiedTime": "2026-10-02T00:00:00Z",
        "version": version,
        "trashed": trashed,
        "parents": [parent] if parent else [],
        **({"driveId": drive} if drive else {}),
        "owners": [{"displayName": "Ada", "emailAddress": "ada@example.com"}],
        "lastModifyingUser": {"displayName": "Grace", "permissionId": "42"},
    }


def folder(id: str, parent: str | None = None) -> dict:
    return item(id, parent=parent, mime=FOLDER)


@dataclass
class FakeGoogle:
    files: dict[str, dict] = field(default_factory=dict)
    texts: dict[str, str] = field(default_factory=dict)
    comments: dict[str, list[dict]] = field(default_factory=dict)
    # Statuses to return, in order, before serving a path normally.
    failures: dict[str, list[int]] = field(default_factory=dict)
    token_status: int = 200
    token_error: str = "invalid_grant"
    calls: list[str] = field(default_factory=list)

    def add(self, *items: dict, text: str = "", comments: list[dict] | None = None):
        for entry in items:
            self.files[entry["id"]] = entry
            self.texts[entry["id"]] = text or f"Body of {entry['id']}"
            self.comments[entry["id"]] = comments or []

    def drive(self) -> Drive:
        credentials = Credentials(
            CLIENT,
            StoredToken.model_validate(
                {"refresh_token": REFRESH_TOKEN, "scope": "drive.readonly"}
            ),
            send=self.send,
            sleep=lambda _: None,
        )
        return Drive(credentials, send=self.send, sleep=lambda _: None)

    def send(
        self, request: urllib.request.Request
    ) -> tuple[int, dict[str, str], bytes]:
        url = urllib.parse.urlsplit(request.full_url)
        query = {k: v[0] for k, v in urllib.parse.parse_qs(url.query).items()}
        path = url.path.removeprefix("/drive/v3/")
        self.calls.append(path)
        pending = self.failures.get(path)
        if pending:
            return pending.pop(0), {"retry-after": "1"}, b'{"error":{"code":429}}'
        if url.hostname == "oauth2.googleapis.com":
            assert isinstance(request.data, bytes)
            form = urllib.parse.parse_qs(request.data.decode())
            if self.token_status != 200:
                body = {"error": self.token_error, "error_description": REFRESH_TOKEN}
                return self.token_status, {}, orjson.dumps(body)
            assert form["client_secret"] == [CLIENT_SECRET]
            body = {"access_token": ACCESS_TOKEN, "expires_in": 3600}
            if form["grant_type"] == ["authorization_code"]:
                body |= {
                    "refresh_token": REFRESH_TOKEN,
                    "scope": "https://www.googleapis.com/auth/drive.readonly",
                }
            return 200, {}, orjson.dumps(body)
        assert request.get_header("Authorization") == f"Bearer {ACCESS_TOKEN}"
        if path == "files":
            if query.get("corpora") == "drive":
                found = [
                    f
                    for f in self.files.values()
                    if f.get("driveId") == query["driveId"]
                    and f["mimeType"] == DOCUMENT
                ]
            else:
                match = PARENTS.search(query["q"])
                assert match is not None
                known = match.group(1) in self.files or any(
                    match.group(1) in f["parents"] for f in self.files.values()
                )
                if not known:
                    return 404, {}, b'{"error":{"code":404}}'
                found = [
                    f for f in self.files.values() if match.group(1) in f["parents"]
                ]
            # Two pages exercise pagination.
            if "pageToken" not in query and len(found) > 1:
                return 200, {}, orjson.dumps({"files": found[:1], "nextPageToken": "p"})
            return (
                200,
                {},
                orjson.dumps({"files": found[1:] if "pageToken" in query else found}),
            )
        parts = path.split("/")
        file_id = parts[1]
        if file_id not in self.files:
            return 404, {}, b'{"error":{"code":404}}'
        if len(parts) == 2:
            return 200, {}, orjson.dumps(self.files[file_id])
        if parts[2] == "export":
            assert query["mimeType"] == "text/markdown"
            return 200, {}, self.texts[file_id].encode()
        return 200, {}, orjson.dumps({"comments": self.comments[file_id]})


def store_token(path: Path) -> None:
    save_token(
        path,
        StoredToken.model_validate(
            {"refresh_token": REFRESH_TOKEN, "scope": "drive.readonly"}
        ),
    )
