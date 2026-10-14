"""An in-memory Discord REST API on a local HTTP server that hikari talks to."""

import threading
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import UTC, datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

import orjson

TOKEN = "MTIzNDU2Nzg5MDEyMzQ1Njc4.secret-do-not-log"
EPOCH = 1420070400000  # Discord's snowflake epoch, in Unix milliseconds.
GUILD = "100000000000000000"
CONTENT_INTENT = 1 << 18


def snowflake(at: str, sequence: int = 0) -> str:
    """The ID Discord would assign at an ISO-8601 instant."""
    milliseconds = int(datetime.fromisoformat(at).timestamp() * 1000)
    return str(((milliseconds - EPOCH) << 22) + sequence)


def channel(id: str, name: str, parent: str | None = None) -> dict:
    if parent is None:
        return {
            "id": id,
            "type": 0,
            "guild_id": GUILD,
            "name": name,
            "position": 0,
            "permission_overwrites": [],
            "nsfw": False,
            "parent_id": None,
            "topic": None,
            "last_message_id": None,
            "rate_limit_per_user": 0,
        }
    return {
        "id": id,
        "type": 11,
        "guild_id": GUILD,
        "name": name,
        "parent_id": parent,
        "owner_id": "1",
        "last_message_id": None,
        "rate_limit_per_user": 0,
        "message_count": 1,
        "member_count": 1,
        "flags": 0,
        "thread_metadata": {
            "archived": False,
            "auto_archive_duration": 1440,
            "archive_timestamp": "2026-10-01T00:00:00+00:00",
            "locked": False,
        },
    }


def user(id: str, name: str, *, bot: bool = False, display: str | None = None) -> dict:
    value: dict[str, object] = {
        "id": id,
        "username": name,
        "discriminator": "0",
        "avatar": None,
        "global_name": display,
    }
    if bot:
        value["bot"] = True
    return value


ADA = user("1", "ada", display="Ada Lovelace")
HELPER = user("2", "helper", bot=True)


def message(
    channel: str,
    id: str,
    content: str,
    *,
    author: dict = ADA,
    type: int = 0,
    reply_to: str | None = None,
    reply_channel: str | None = None,
    edited: str | None = None,
    files: list[dict] | None = None,
) -> dict:
    created = (int(id) >> 22) + EPOCH
    value = {
        "id": id,
        "channel_id": channel,
        "author": author,
        "content": content,
        "timestamp": datetime.fromtimestamp(created / 1000, UTC).isoformat(),
        "edited_timestamp": edited,
        "tts": False,
        "mention_everyone": False,
        "mentions": [],
        "mention_roles": [],
        "attachments": files or [],
        "embeds": [],
        "pinned": False,
        "type": type,
        "flags": 0,
    }
    if reply_to is not None:
        value["message_reference"] = {
            "type": 0,
            "message_id": reply_to,
            "channel_id": reply_channel or channel,
            "guild_id": GUILD,
        }
    return value


def attachment(id: str, name: str) -> dict:
    url = f"https://cdn.discordapp.com/attachments/1/{id}/{name}"
    return {
        "id": id,
        "filename": name,
        "size": 12,
        "url": url,
        "proxy_url": url,
        "content_type": "text/plain",
    }


def application(flags: int) -> dict:
    return {
        "id": "9",
        "name": "mash",
        "description": "",
        "bot_public": False,
        "bot_require_code_grant": False,
        "owner": ADA,
        "verify_key": "00",
        "flags": flags,
        "approximate_guild_count": 1,
        "approximate_user_install_count": 0,
    }


@dataclass
class FakeDiscord:
    channels: dict[str, dict] = field(default_factory=dict)
    messages: dict[str, list[dict]] = field(default_factory=dict)
    flags: int = CONTENT_INTENT
    # Responses (status, JSON body) to return, in order, before serving a path.
    failures: dict[str, list[tuple[int, dict]]] = field(default_factory=dict)
    calls: list[str] = field(default_factory=list)
    authorizations: set[str] = field(default_factory=set)

    def add(self, info: dict, *items: dict) -> None:
        self.channels[info["id"]] = info
        self.messages.setdefault(info["id"], []).extend(items)

    def handle(self, path: str, query: dict[str, str]) -> tuple[int, object]:
        pending = self.failures.get(path)
        if pending:
            return pending.pop(0)
        parts = path.removeprefix("/api/v10/").split("/")
        if parts == ["oauth2", "applications", "@me"]:
            return 200, application(self.flags)
        if parts[0] != "channels" or parts[1] not in self.channels:
            return 404, {"message": "Unknown Channel", "code": 10003}
        if len(parts) == 2:
            return 200, self.channels[parts[1]]
        # Discord returns the page right after `after`, newest first.
        after = int(query.get("after", "0"))
        found = sorted(
            (item for item in self.messages[parts[1]] if int(item["id"]) > after),
            key=lambda item: int(item["id"]),
        )[: int(query.get("limit", "50"))]
        return 200, found[::-1]

    @contextmanager
    def serve(self) -> Iterator[str]:
        """The API base URL, while a background server answers it."""
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                fake.authorizations.add(self.headers["Authorization"])
                url = urlsplit(self.path)
                fake.calls.append(self.path)
                query = {key: values[0] for key, values in parse_qs(url.query).items()}
                status, body = fake.handle(url.path, query)
                data = orjson.dumps(body)
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, format: str, *args: object) -> None:
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(
            target=server.serve_forever, args=(0.01,), daemon=True
        )
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_address[1]}/api/v10"
        finally:
            server.shutdown()
            server.server_close()
