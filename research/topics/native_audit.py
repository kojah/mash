"""Bounded native-file identity/content audit, without exposing source text."""
import argparse
from collections import Counter
from dataclasses import asdict, dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import sqlite3

import blake3

from episodes import Turn, read_cases, source_hash
from preceding_context import sha


class Provider(StrEnum):
    CLAUDE = "claude-code"
    CODEX = "codex"


class Match(StrEnum):
    VERIFIED = "identity-role-canonical-content"
    MISSING = "identity-not-found"
    CONFLICT = "conflicting-native-identity"
    CONTENT = "content-or-role-mismatch"
    UNSUPPORTED = "unsupported-canonical-content"


@dataclass(frozen=True)
class Native:
    line: int
    record_sha256: str
    uuid: str | None
    stored_id: str | None
    kind: str
    role: str | None
    content_sha256: str | None
    parent_present: bool
    parent: str | None
    origin_fields: tuple[str, ...]
    human_origin: bool


def stable_id(domain: str, parts: tuple[str, ...]) -> str:
    return blake3.blake3(domain.encode() + b"".join(b"\0" + part.encode() for part in parts)).hexdigest()[:24]


def plain_content(value) -> str | None:
    """Exact canonical text subset; never synthesize tool/thinking contents."""
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        if isinstance(value.get("text"), str):
            return value["text"]
        if isinstance(value.get("content"), str):
            return value["content"]
        return None
    if isinstance(value, list):
        # Image-only blocks contribute no text; explicit empty text blocks still
        # contribute a separator, matching the canonical ingestion projection.
        parts = [plain_content(part) for part in value
                 if not (isinstance(part, dict) and part.get("type") in ("input_image", "image")
                         and not isinstance(part.get("text"), str) and not isinstance(part.get("content"), str))]
        return "\n".join(parts) if all(part is not None for part in parts) else None
    return None


def decode(line: bytes, number: int, provider: Provider, path: Path, thread: str) -> Native:
    try:
        raw = json.loads(line)
    except (ValueError, UnicodeError):
        raise ValueError(f"invalid native JSON at line {number}") from None
    if not isinstance(raw, dict):
        raise ValueError(f"invalid native envelope at line {number}")
    kind = raw.get("type")
    if not isinstance(kind, str):
        raise ValueError("invalid native record type")
    uuid = raw.get("uuid") if provider == Provider.CLAUDE else None
    if uuid is not None and not isinstance(uuid, str):
        raise ValueError("invalid native UUID")
    parent = raw.get("parentUuid")
    if parent is not None and not isinstance(parent, str):
        raise ValueError("invalid native parent UUID")
    message = raw.get("message") if provider == Provider.CLAUDE and kind in ("user", "assistant") else (
        raw.get("payload") if provider == Provider.CODEX and kind == "response_item" else None)
    role, content, stored_id = None, None, None
    if isinstance(message, dict) and (provider == Provider.CLAUDE or message.get("type") == "message"):
        role = message.get("role", kind if provider == Provider.CLAUDE else None)
        content = plain_content(message.get("content"))
        if provider == Provider.CLAUDE and uuid is not None:
            stored_id = stable_id("claude-message", (thread, uuid))
        elif provider == Provider.CODEX and isinstance(message.get("id"), str):
            stored_id = stable_id("message", (provider, str(path), message["id"]))
    if provider == Provider.CODEX and kind == "event_msg":
        payload = raw.get("payload")
        item = payload.get("item") if isinstance(payload, dict) and payload.get("type") == "item_completed" else None
        if isinstance(item, dict) and item.get("type") in ("UserMessage", "AgentMessage"):
            role = "user" if item["type"] == "UserMessage" else "assistant"
            text_kind = "text" if role == "user" else "Text"
            blocks = item.get("content")
            if isinstance(blocks, list) and all(isinstance(block, dict) for block in blocks):
                known = [block for block in blocks if block.get("type") == text_kind]
                if all(isinstance(block.get("text"), str) for block in known):
                    content = "\n".join(block["text"] for block in known)
            if isinstance(payload.get("turn_id"), str) and isinstance(item.get("id"), str):
                stored_id = stable_id("message", (provider, str(path), f"completed:{payload['turn_id']}:{item['id']}"))
    if role is not None and not isinstance(role, str):
        raise ValueError("invalid native role")
    return Native(number, hashlib.sha256(line).hexdigest(), uuid, stored_id, str(kind), role,
                  sha(content) if content is not None else None, "parentUuid" in raw, parent,
                  tuple(key for key in ("origin", "promptSource", "turnOrigin", "userType") if key in raw),
                  raw.get("turnOrigin") == "human")


def read_native(path: Path, provider: Provider, thread: str) -> tuple[str, tuple[Native, ...]]:
    with path.open("rb") as source:
        raw = source.read(64 * 1024 * 1024 + 1)
    if len(raw) > 64 * 1024 * 1024:
        raise ValueError("native source exceeds 64 MiB audit cap")
    lines = raw.splitlines(keepends=True)
    if len(lines) > 100000 or any(len(line) > 8 * 1024 * 1024 for line in lines):
        raise ValueError("native source exceeds record audit cap")
    records = tuple(decode(line, i, provider, path, thread) for i, line in enumerate(lines, 1) if line.strip())
    # Detect append/edit races. Audit exact bytes, not a claim of historical file identity.
    with path.open("rb") as source:
        repeated = source.read(len(raw) + 1)
    if raw != repeated:
        raise ValueError("native source changed during audit")
    return hashlib.sha256(raw).hexdigest(), records


def verify(records: tuple[Native, ...], identity: str, role: str, content: str) -> dict:
    candidates = [r for r in records if r.stored_id == identity]
    status = Match.MISSING
    if candidates:
        signatures = {(r.role, r.content_sha256, r.parent_present, r.parent) for r in candidates}
        if len(signatures) != 1:
            status = Match.CONFLICT
        elif candidates[0].content_sha256 is None:
            status = Match.UNSUPPORTED
        elif candidates[0].role != role or candidates[0].content_sha256 != sha(content):
            status = Match.CONTENT
        else:
            status = Match.VERIFIED
    return {"status": status, "native_records": [asdict(r) for r in candidates]}


def parent_evidence(records: tuple[Native, ...], native: Native, thread: str,
                    stored_ordinals: dict[str, int], expected: int | None) -> str:
    by_uuid = {}
    for record in records:
        if record.uuid is not None:
            by_uuid.setdefault(record.uuid, []).append(record)
    current = native
    seen = {native.uuid} if native.uuid is not None else set()
    for _ in range(64):
        if not current.parent_present:
            return "missing-native-parent-field"
        if current.parent is None:
            return "verified-root" if expected == -1 else "parent-mismatch"
        if current.parent in seen:
            return "native-parent-cycle"
        seen.add(current.parent)
        candidates = by_uuid.get(current.parent, [])
        if not candidates:
            return "missing-native-parent-record"
        if len({(r.kind, r.parent_present, r.parent, r.stored_id, r.content_sha256) for r in candidates}) != 1:
            return "conflicting-native-parent-record"
        current = candidates[0]
        identity = stable_id("claude-message", (thread, current.uuid))
        if identity in stored_ordinals:
            return "verified-parent" if stored_ordinals[identity] == expected else "parent-mismatch"
        if current.kind in ("user", "assistant"):
            return "unrepresented-native-message"
    return "native-parent-limit"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private audit must stay outside the repository")
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    audited = []
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("BEGIN")
        for case in cases:
            provider = Provider(case.provider)
            path = Path(connection.execute("SELECT source_path FROM conversations WHERE id=?", (case.thread,)).fetchone()[0])
            rows = connection.execute("SELECT id,ordinal,role,content,context_parent,COALESCE(search_projection,content) "
                                      "FROM messages WHERE conversation_id=? ORDER BY ordinal", (case.thread,)).fetchall()
            visible = tuple(Turn(r[0], r[1], r[5], None) for r in rows if r[2] == "user" and r[5].strip())
            if source_hash(visible) != case.source_sha256:
                raise ValueError("annotation source changed before native capture")
            source_sha, records = read_native(path, provider, case.thread)
            ordinals = {r[0]: r[1] for r in rows}
            results = []
            for identity, ordinal, role, canonical, parent, projection in rows:
                if role != "user" or not projection.strip():
                    continue
                result = verify(records, identity, role, canonical)
                result.update(id=identity, ordinal=ordinal, searchable_sha256=sha(projection))
                result["parent_evidence"] = (parent_evidence(records, next(r for r in records if r.stored_id == identity),
                                                           case.thread, ordinals, parent)
                                             if provider == Provider.CLAUDE and result["status"] == Match.VERIFIED
                                             else "not-verified")
                results.append(result)
            audited.append({"thread": case.thread, "provider": provider, "native_source_sha256": source_sha,
                            "source_sha256": case.source_sha256, "records": results})
    report = {"schema_version": 1, "dataset_fingerprint": fingerprint, "cases": audited,
              "annotation_sha256": sha(args.annotations.read_text()), "code_sha256": sha(Path(__file__).read_text()),
              "lock_sha256": sha(Path(__file__).with_name("uv.lock").read_text()),
              "limits": "64 MiB/file;8 MiB/record;100000 records;64 parent steps",
              "limitations": ["canonical text subset only; unsupported tool/thinking bodies are not guessed",
                              "Codex explicit raw/completed IDs only; no fallback ordinal or mirror reconciliation",
                              "role or human-origin metadata does not prove substantive user intent",
                              "current native bytes pinned, not historical whole-file equivalence",
                              "no semantic referent or topic-quality judgments"]}
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output, ensure_ascii=False, indent=2)
    print(json.dumps(dict(Counter(r["status"] for c in audited for r in c["records"]))))


if __name__ == "__main__":
    main()
