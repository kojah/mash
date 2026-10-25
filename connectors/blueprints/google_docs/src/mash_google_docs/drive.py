"""The Drive v3 calls this connector needs, with typed responses."""

import time
import urllib.parse
import urllib.request
from collections.abc import Iterator
from datetime import datetime

from pydantic import BaseModel, ConfigDict
from pydantic.alias_generators import to_camel

from .api import HttpError, Send, Sleep, fetch
from .auth import Credentials

API = "https://www.googleapis.com/drive/v3"
DOCUMENT = "application/vnd.google-apps.document"
FOLDER = "application/vnd.google-apps.folder"
USER_FIELDS = "displayName,emailAddress,permissionId"
FILE_FIELDS = (
    "id,name,mimeType,createdTime,modifiedTime,version,trashed,parents,driveId,"
    f"owners({USER_FIELDS}),lastModifyingUser({USER_FIELDS})"
)
COMMENT_FIELDS = (
    f"id,author({USER_FIELDS}),content,createdTime,modifiedTime,resolved,deleted,"
    f"quotedFileContent,replies(id,author({USER_FIELDS}),content,createdTime,"
    "modifiedTime,deleted,action)"
)


class Model(BaseModel):
    # Google adds fields over time; unknown ones are ignored, not rejected.
    model_config = ConfigDict(
        alias_generator=to_camel, populate_by_name=True, extra="ignore", frozen=True
    )


class User(Model):
    display_name: str | None = None
    email_address: str | None = None
    permission_id: str | None = None


class File(Model):
    id: str
    name: str
    mime_type: str
    created_time: datetime
    modified_time: datetime
    # Increases with every server-side change to the file, not only edits.
    version: str
    trashed: bool = False
    parents: list[str] = []
    drive_id: str | None = None
    owners: list[User] = []
    last_modifying_user: User | None = None


class Quoted(Model):
    value: str | None = None


class Reply(Model):
    id: str
    author: User | None = None
    content: str = ""
    created_time: datetime | None = None
    modified_time: datetime | None = None
    deleted: bool = False
    action: str | None = None


class Comment(Model):
    id: str
    author: User | None = None
    content: str = ""
    created_time: datetime | None = None
    modified_time: datetime | None = None
    resolved: bool = False
    deleted: bool = False
    quoted_file_content: Quoted | None = None
    replies: list[Reply] = []


class FilePage(Model):
    files: list[File] = []
    next_page_token: str | None = None


class CommentPage(Model):
    comments: list[Comment] = []
    next_page_token: str | None = None


class Drive:
    def __init__(
        self, credentials: Credentials, *, send: Send, sleep: Sleep = time.sleep
    ) -> None:
        self.credentials = credentials
        self.send = send
        self.sleep = sleep

    def get(self, path: str, query: dict[str, str]) -> bytes:
        url = f"{API}/{path}?{urllib.parse.urlencode(query)}"
        for attempt in range(2):
            request = urllib.request.Request(
                url, headers={"Authorization": self.credentials.header()}
            )
            try:
                return fetch(self.send, self.sleep, request)
            except HttpError as error:
                # An access token can be revoked before it expires; refresh once.
                if error.status != 401 or attempt:
                    raise
                self.credentials.refresh()
        raise AssertionError("unreachable")

    def files(self, query: dict[str, str]) -> Iterator[File]:
        page_token = None
        while True:
            page = FilePage.model_validate_json(
                self.get(
                    "files",
                    {
                        **query,
                        "fields": f"nextPageToken,files({FILE_FIELDS})",
                        "pageSize": "1000",
                        "supportsAllDrives": "true",
                        "includeItemsFromAllDrives": "true",
                        **({"pageToken": page_token} if page_token else {}),
                    },
                )
            )
            yield from page.files
            if page.next_page_token is None:
                return
            page_token = page.next_page_token

    def children(self, folder_id: str) -> Iterator[File]:
        """Docs and subfolders directly in a folder, trashed ones included."""
        return self.files(
            {
                "q": f"'{folder_id}' in parents and "
                f"(mimeType='{DOCUMENT}' or mimeType='{FOLDER}')",
                "corpora": "allDrives",
            }
        )

    def shared_drive(self, drive_id: str) -> Iterator[File]:
        return self.files(
            {"q": f"mimeType='{DOCUMENT}'", "corpora": "drive", "driveId": drive_id}
        )

    def file(self, file_id: str) -> File:
        return File.model_validate_json(
            self.get(
                f"files/{urllib.parse.quote(file_id, safe='')}",
                {"fields": FILE_FIELDS, "supportsAllDrives": "true"},
            )
        )

    def export(self, file_id: str) -> str:
        path = f"files/{urllib.parse.quote(file_id, safe='')}/export"
        try:
            body = self.get(path, {"mimeType": "text/markdown"})
        except HttpError as error:
            # Markdown export is newer than plain text; fall back if refused.
            if error.status != 400:
                raise
            body = self.get(path, {"mimeType": "text/plain"})
        return body.decode("utf-8", errors="replace")

    def comments(self, file_id: str) -> list[Comment]:
        path = f"files/{urllib.parse.quote(file_id, safe='')}/comments"
        found: list[Comment] = []
        page_token = None
        while True:
            query = {"fields": f"nextPageToken,comments({COMMENT_FIELDS})"}
            query["pageSize"] = "100"
            if page_token:
                query["pageToken"] = page_token
            page = CommentPage.model_validate_json(self.get(path, query))
            found.extend(page.comments)
            if page.next_page_token is None:
                return found
            page_token = page.next_page_token
