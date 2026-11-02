"""An in-memory Slack Web API served over real HTTP from a background thread."""

import asyncio
import threading
from dataclasses import dataclass, field
from decimal import Decimal

from aiohttp import web
from slack_sdk.web.async_client import AsyncWebClient

from mash_slack.client import connect

TOKEN = "xoxb-secret-do-not-log"
TEAM = "T0TEAM"
URL = "https://acme.slack.com/"


def ts(seconds: int, micros: int = 0) -> str:
    return f"{seconds}.{micros:06d}"


def post(
    stamp: str, text: str, user: str | None = "U1", **values: object
) -> dict[str, object]:
    message: dict[str, object] = {"type": "message", "ts": stamp, "text": text}
    if user is not None:
        message["user"] = user
    return message | values


@dataclass
class FakeSlack:
    channels: dict[str, dict] = field(default_factory=dict)
    # Every message of a channel, top-level and replies, in any order.
    messages: dict[str, list[dict]] = field(default_factory=dict)
    users: dict[str, dict] = field(default_factory=dict)
    # Responses to return, in order, before serving a method normally.
    failures: dict[str, list[web.Response]] = field(default_factory=dict)
    calls: list[tuple[str, dict[str, str]]] = field(default_factory=list)
    page: int = 2
    loop: asyncio.AbstractEventLoop = field(init=False)
    thread: threading.Thread = field(init=False)
    runner: web.AppRunner = field(init=False)
    base_url: str = field(init=False)

    def channel(self, id: str, name: str, **values: object) -> None:
        self.channels[id] = {"id": id, "name": name, "created": 1700000000} | values
        self.messages.setdefault(id, [])

    def user(self, id: str, display: str, **values: object) -> None:
        self.users[id] = {
            "id": id,
            "name": display.lower(),
            "profile": {"display_name": display, "real_name": f"{display} Real"},
        } | values

    def __enter__(self) -> "FakeSlack":
        self.loop = asyncio.new_event_loop()
        started = threading.Event()

        async def serve() -> None:
            app = web.Application()
            app.router.add_route("*", "/api/{method}", self.handle)
            self.runner = web.AppRunner(app)
            await self.runner.setup()
            site = web.TCPSite(self.runner, "127.0.0.1", 0)
            await site.start()
            port = self.runner.addresses[0][1]
            self.base_url = f"http://127.0.0.1:{port}/api/"
            started.set()

        self.thread = threading.Thread(target=self.loop.run_forever, daemon=True)
        self.thread.start()
        asyncio.run_coroutine_threadsafe(serve(), self.loop)
        started.wait(10)
        return self

    def __exit__(self, *_: object) -> None:
        asyncio.run_coroutine_threadsafe(self.runner.cleanup(), self.loop).result(10)
        self.loop.call_soon_threadsafe(self.loop.stop)
        self.thread.join(10)

    def client(self) -> AsyncWebClient:
        return connect(TOKEN, self.base_url)

    async def handle(self, request: web.Request) -> web.Response:
        assert request.headers["authorization"] == f"Bearer {TOKEN}"
        method = request.match_info["method"]
        query = dict(request.query) | {
            k: str(v) for k, v in (await request.post()).items()
        }
        self.calls.append((method, query))
        pending = self.failures.get(method)
        if pending:
            return pending.pop(0)
        match method:
            case "auth.test":
                return ok(team_id=TEAM, user_id="U0ALICE", team="Acme", url=URL)
            case "users.info":
                if query["user"] not in self.users:
                    return ok_false("user_not_found")
                return ok(user=self.users[query["user"]])
            case "conversations.info":
                if query["channel"] not in self.channels:
                    return ok_false("channel_not_found")
                return ok(channel=self.channels[query["channel"]])
            case "conversations.history":
                oldest = Decimal(query.get("oldest", "0"))
                found = [
                    self.summary(query["channel"], m)
                    for m in self.messages[query["channel"]]
                    if m.get("thread_ts") in (None, m["ts"])
                    or m.get("subtype") == "thread_broadcast"
                ]
                found = [m for m in found if Decimal(m["ts"]) > oldest]
                found.sort(key=lambda m: Decimal(m["ts"]), reverse=True)
                return self.paged(found, query)
            case "conversations.replies":
                parent = query["ts"]
                found = [
                    self.summary(query["channel"], m)
                    for m in self.messages[query["channel"]]
                    if m["ts"] == parent or m.get("thread_ts") == parent
                ]
                found.sort(key=lambda m: Decimal(m["ts"]))
                return self.paged(found, query)
        return ok_false("unknown_method")

    def summary(self, channel: str, message: dict) -> dict:
        """Thread parents carry reply_count and latest_reply, as Slack's do."""
        replies = [
            m["ts"]
            for m in self.messages[channel]
            if m.get("thread_ts") == message["ts"] and m["ts"] != message["ts"]
        ]
        if not replies:
            return message
        latest = max(replies, key=Decimal)
        return message | {
            "thread_ts": message["ts"],
            "reply_count": len(replies),
            "latest_reply": latest,
        }

    def paged(self, found: list[dict], query: dict[str, str]) -> web.Response:
        start = int(query.get("cursor") or 0)
        size = min(int(query.get("limit", 100)), self.page)
        following = start + size if start + size < len(found) else None
        return ok(
            messages=found[start : start + size],
            has_more=following is not None,
            response_metadata={"next_cursor": str(following or "")},
        )


def ok(**values: object) -> web.Response:
    return web.json_response({"ok": True, **values})


def ok_false(error: str) -> web.Response:
    return web.json_response({"ok": False, "error": error})


def limited() -> web.Response:
    return web.json_response(
        {"ok": False, "error": "ratelimited"}, status=429, headers={"Retry-After": "0"}
    )
