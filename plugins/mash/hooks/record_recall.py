"""Bounded, dependency-free MCP client for the hooks' local record context.

The host supplies only Python's standard library. Rust owns selection, privacy
and ranking; this client only validates and renders the returned summaries.
"""

from __future__ import annotations

import asyncio
import json
import os
import subprocess
from dataclasses import dataclass
from typing import Any

PROTOCOL = "2025-06-18"
MAX_LINE = 65536
KINDS = {"decision", "preference", "fact", "open_question"}


@dataclass(frozen=True)
class Hint:
    record_id: str
    revision: int
    kind: str
    statement: str

    @classmethod
    def parse(cls, value: Any) -> Hint:
        if not isinstance(value, dict) or value.get("status") != "current":
            raise ValueError("recall returned a non-current record")
        body = value.get("body")
        if not isinstance(body, dict) or body.get("review") != "established":
            raise ValueError("recall returned an unreviewed record")
        details = body.get("details")
        kind = details.get("kind") if isinstance(details, dict) else None
        record_id, revision = value.get("id"), value.get("revision")
        statement = body.get("statement")
        if (
            kind not in KINDS
            or not isinstance(record_id, str)
            or not 0 < len(record_id) <= 512
            or type(revision) is not int
            or revision < 1
            or not isinstance(statement, str)
            or not 0 < len(statement) <= 4096
        ):
            raise ValueError("invalid recall item")
        return cls(record_id, revision, kind, statement)

    def line(self) -> str:
        statement = " ".join(self.statement.split())
        if len(statement) > 220:
            statement = statement[:219] + "…"
        lookup = json.dumps({"id": self.record_id}, separators=(",", ":"))
        return f"- {self.kind}: {statement} get_record={lookup}"


async def reply(process: asyncio.subprocess.Process, expected: int) -> dict[str, Any]:
    assert process.stdout is not None
    for _ in range(16):
        line = await process.stdout.readline()
        if not line or len(line) > MAX_LINE:
            raise ValueError("missing or oversized MCP response")
        value = json.loads(line)
        if not isinstance(value, dict) or value.get("jsonrpc") != "2.0":
            raise ValueError("invalid MCP response")
        if value.get("id") != expected:
            if "id" in value:
                raise ValueError("unexpected MCP response or request")
            continue  # Bounded notifications, within the overall deadline.
        result = value.get("result")
        if "error" in value or not isinstance(result, dict):
            raise ValueError("MCP request failed")
        return result
    raise ValueError("too many MCP notifications")


async def exchange(process: asyncio.subprocess.Process, arguments: dict[str, Any]):
    writer = process.stdin
    assert writer is not None

    async def send(message: dict[str, Any]) -> None:
        writer.write(json.dumps({"jsonrpc": "2.0", **message}).encode() + b"\n")
        await writer.drain()

    await send(
        {
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": {"name": "mash-recall-hook", "version": "1"},
            },
        }
    )
    initialized = await reply(process, 1)
    if initialized.get("protocolVersion") != PROTOCOL:
        raise ValueError("unsupported MCP protocol")
    await send({"method": "notifications/initialized"})
    await send(
        {
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "recall_records",
                "arguments": arguments,
            },
        }
    )
    result = await reply(process, 2)
    if result.get("isError"):
        raise ValueError("record recall failed")
    packet = result.get("structuredContent")
    if not isinstance(packet, dict) or not isinstance(packet.get("records"), list):
        raise ValueError("missing structured recall response")
    if len(packet["records"]) > 3 or len(json.dumps(packet).encode()) > 16384:
        raise ValueError("oversized recall packet")
    hints = [Hint.parse(item).line() for item in packet["records"]]
    if hints and (packet.get("scan_limited") or packet.get("truncated")):
        hints.append("- More records may exist; use list_records to inspect them.")
    return hints


async def fetch(binary: str, arguments: dict[str, Any], timeout: float) -> list[str]:
    process = await asyncio.create_subprocess_exec(
        binary,
        "mcp",
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        limit=MAX_LINE,
    )
    try:
        return await asyncio.wait_for(exchange(process, arguments), timeout)
    finally:
        if process.returncode is None:
            try:
                process.kill()
            except ProcessLookupError:
                pass
        await process.wait()


def hints(
    binary: str,
    project: str,
    repository: str | None,
    timeout: float,
    file: str | None = None,
) -> list[str]:
    """Fail open within a small share of the caller's remaining deadline."""
    if os.environ.get("MASH_RECORD_RECALL", "").lower().strip() in (
        "0",
        "off",
        "false",
    ):
        return []
    if timeout <= 0:
        return []
    scope = (
        {"kind": "repository", "identity": repository}
        if repository
        else {"kind": "local_project", "path": project}
    )
    arguments: dict[str, Any] = {"scope": scope, "limit": 3, "byte_budget": 4096}
    if file is not None:
        arguments["file"] = file
    try:
        return asyncio.run(fetch(binary, arguments, min(timeout, 0.75)))
    except (OSError, ValueError, TypeError, KeyError, asyncio.TimeoutError):
        return []
