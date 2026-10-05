"""Freeze complete silver ancestry, retaining experimental source identities."""

import gzip
import hashlib
import os
import sqlite3
from enum import StrEnum
from pathlib import Path

import blake3
import click
import orjson
from pydantic import BaseModel, ConfigDict, Field

from corpus import Corpus, Provider, Record, Split, Thread
from turns import AssistantActivity, Turn, extract, injected


class CanonicalRole(StrEnum):
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"
    SYSTEM = "system"
    DEVELOPER = "developer"


class Canonical(Record):
    id: str
    ordinal: int = Field(ge=0)
    role: CanonicalRole
    context_parent: int | None
    projection: str
    content_sha256: str
    created_at: int | None


class Resolution(Record):
    source: int
    old_parent: int | None
    parent: int | None
    skipped: tuple[int, ...]


class CapturedThread(Record):
    index: int
    thread: Thread
    canonical: tuple[Canonical, ...]
    resolutions: tuple[Resolution, ...]
    activities: tuple[AssistantActivity, ...]
    original_turns: tuple[Turn, ...]
    bridged_turns: tuple[Turn, ...]
    turns: tuple[Turn, ...]
    source_sha256: str
    native_matched: int
    native_parent_checked: int
    native_parent_mismatches: int
    native_compactions: int


class Capture(Record):
    schema_version: int = 1
    original_corpus_sha256: str
    split: Split = Split.DEVELOPMENT
    threads: tuple[CapturedThread, ...]


class NativeBlock(BaseModel):
    model_config = ConfigDict(extra="ignore")
    type: str = "unknown"
    text: str | None = None


class NativeMessage(BaseModel):
    model_config = ConfigDict(extra="ignore")
    type: str | None = None
    role: str | None = None
    content: str | tuple[NativeBlock, ...] | None = None

    def text(self) -> str | None:
        if isinstance(self.content, str):
            return self.content
        if self.content is None:
            return None
        texts = [
            block.text
            for block in self.content
            if block.type in ("text", "input_text", "output_text")
            and block.text is not None
        ]
        return "\n".join(texts) if texts else None


class NativeRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    type: str
    uuid: str | None = None
    parentUuid: str | None = None
    subtype: str | None = None
    message: NativeMessage | None = None
    payload: NativeMessage | None = None


def stable_id(domain: str, parts: tuple[str, ...]) -> str:
    return blake3.blake3(
        domain.encode() + b"".join(b"\0" + part.encode() for part in parts)
    ).hexdigest()[:24]


def bridge(
    thread: Thread, canonical: tuple[Canonical, ...]
) -> tuple[Thread, tuple[Resolution, ...], tuple[AssistantActivity, ...]]:
    """Collapse only transparent records; never guess a missing explicit parent.

    Canonical role/activity matters even when its projection has no embedding.
    Linear records follow the previous canonical record; explicit roots stay roots.
    """
    ordinals = [row.ordinal for row in canonical]
    if ordinals != sorted(set(ordinals)) or len({row.id for row in canonical}) != len(
        canonical
    ):
        raise ValueError("canonical identities/ordinals must be unique and ordered")
    by_ordinal = {row.ordinal: row for row in canonical}
    canonical_visible = {
        row.id
        for row in canonical
        if row.role in (CanonicalRole.USER, CanonicalRole.ASSISTANT)
        and row.projection.strip()
    }
    if canonical_visible != {
        message.id for message in thread.messages if message.projection.strip()
    }:
        raise ValueError("frozen corpus omits visible canonical dialogue")
    eligible = {
        message.ordinal
        for message in thread.messages
        if message.projection.strip() and not injected(message)
    }
    collapsed: dict[int, int] = {}
    paths: dict[int, tuple[int, ...]] = {}
    previous = -1
    for row in canonical:
        parent = previous if row.context_parent is None else row.context_parent
        if parent != -1 and (parent not in by_ordinal or parent >= row.ordinal):
            raise ValueError("canonical parent is missing or not earlier")
        if parent == -1 or parent in eligible:
            collapsed[row.ordinal] = parent
            paths[row.ordinal] = ()
        else:
            collapsed[row.ordinal] = collapsed[parent]
            paths[row.ordinal] = (parent,) + paths[parent]
        previous = row.ordinal
    messages = []
    resolutions = []
    for index, message in enumerate(thread.messages):
        row = by_ordinal.get(message.ordinal)
        if row is None or (
            row.id,
            row.role.value,
            row.projection,
            row.context_parent,
        ) != (
            message.id,
            message.role.value,
            message.projection,
            message.context_parent,
        ):
            raise ValueError("canonical history differs from frozen message")
        parent = None if row.context_parent is None else collapsed[row.ordinal]
        messages.append(message.model_copy(update={"context_parent": parent}))
        resolutions.append(
            Resolution(
                source=index,
                old_parent=message.context_parent,
                parent=parent,
                skipped=paths[row.ordinal],
            )
        )
    activities = tuple(
        AssistantActivity(
            ordinal=row.ordinal,
            parent=None if row.context_parent is None else collapsed[row.ordinal],
        )
        for row in canonical
        if row.role == CanonicalRole.ASSISTANT and row.ordinal not in eligible
    )
    return (
        thread.model_copy(update={"messages": tuple(messages)}),
        tuple(resolutions),
        activities,
    )


def native_check(
    thread: Thread, canonical: tuple[Canonical, ...], raw: bytes
) -> tuple[int, int, int, int]:
    """Independent source evidence, without reproducing the production parser.

    Claude UUIDs align directly; verify resolvable raw parent chains. Compaction
    can rewrite logical ancestry and is reported rather than guessed. Codex text
    fingerprints establish presence only, not duplicate-event reconciliation.
    """
    records = tuple(
        NativeRecord.model_validate_json(line)
        for line in raw.splitlines()
        if line.strip()
    )
    compactions = sum(record.subtype == "compact_boundary" for record in records)
    by_id = {row.id: row for row in canonical}
    if thread.provider == Provider.CODEX:
        visible_texts = {
            (record.payload.role, hashlib.sha256(text.encode()).hexdigest())
            for record in records
            if record.type == "response_item"
            and record.payload is not None
            and record.payload.type == "message"
            and (text := record.payload.text()) is not None
        }
        return (
            sum(
                (
                    message.role.value,
                    hashlib.sha256(message.projection.encode()).hexdigest(),
                )
                in visible_texts
                or (message.role.value, by_id[message.id].content_sha256)
                in visible_texts
                for message in thread.messages
            ),
            0,
            0,
            compactions,
        )
    native: dict[str, NativeRecord] = {}
    for record in records:
        if record.uuid is None:
            continue
        previous = native.get(record.uuid)
        if previous is not None and previous.parentUuid != record.parentUuid:
            raise ValueError("conflicting native parents for one UUID")
        native[record.uuid] = record
    mapped = {uid: stable_id("claude-message", (thread.id, uid)) for uid in native}
    found = {identity: uid for uid, identity in mapped.items() if identity in by_id}
    checked = mismatches = 0
    for identity, uid in found.items():
        record = native[uid]
        if "parentUuid" not in record.model_fields_set:
            continue
        parent = record.parentUuid
        seen: set[str] = set()
        while parent is not None and mapped.get(parent) not in by_id:
            if parent in seen:
                raise ValueError("native ancestry cycle")
            seen.add(parent)
            ancestor = native.get(parent)
            if ancestor is None or "parentUuid" not in ancestor.model_fields_set:
                break
            parent = ancestor.parentUuid
        if parent is not None and mapped.get(parent) not in by_id:
            continue
        expected = -1 if parent is None else by_id[mapped[parent]].ordinal
        checked += 1
        mismatches += int(by_id[identity].context_parent != expected)
    return len(found), checked, mismatches, compactions


@click.command()
@click.option("--database", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--corpus", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output-dir", type=click.Path(path_type=Path), required=True)
def main(database: Path, corpus: Path, output_dir: Path) -> None:
    if not output_dir.resolve().is_relative_to(
        Path("/home/james/scratch/mash-artifacts")
    ):
        raise click.ClickException("use private artifact directory")
    output_dir.mkdir(mode=0o700, parents=True, exist_ok=False)
    os.umask(0o077)
    encoded = corpus.read_bytes()
    source = Corpus.model_validate_json(encoded)
    threads = []
    with sqlite3.connect(f"{database.resolve().as_uri()}?mode=ro", uri=True) as db:
        db.row_factory = sqlite3.Row
        db.execute("BEGIN")
        for index, entry in enumerate(source.selected):
            if entry.split != Split.DEVELOPMENT:
                continue
            original = entry.thread
            rows = db.execute(
                """SELECT id,ordinal,role,context_parent,
                    COALESCE(search_projection,content) AS projection,
                    content,created_at FROM messages
                    WHERE conversation_id=? ORDER BY ordinal""",
                (original.id,),
            ).fetchall()
            canonical = tuple(
                Canonical(
                    id=row["id"],
                    ordinal=row["ordinal"],
                    role=CanonicalRole(row["role"]),
                    context_parent=row["context_parent"],
                    projection=row["projection"],
                    content_sha256=hashlib.sha256(row["content"].encode()).hexdigest(),
                    created_at=row["created_at"],
                )
                for row in rows
            )
            thread, resolutions, activities = bridge(original, canonical)
            raw = original.source_path.read_bytes()
            matched, checked, mismatches, compactions = native_check(
                original, canonical, raw
            )
            with gzip.open(output_dir / f"source-{index}.jsonl.gz", "wb") as file:
                file.write(raw)
            threads.append(
                CapturedThread(
                    index=index,
                    thread=thread,
                    canonical=canonical,
                    resolutions=resolutions,
                    activities=activities,
                    original_turns=extract(original),
                    bridged_turns=extract(thread),
                    turns=extract(thread, activities),
                    source_sha256=hashlib.sha256(raw).hexdigest(),
                    native_matched=matched,
                    native_parent_checked=checked,
                    native_parent_mismatches=mismatches,
                    native_compactions=compactions,
                )
            )
            click.echo(f"Captured development thread {index}: {len(canonical)} records")
    capture = Capture(
        original_corpus_sha256=hashlib.sha256(encoded).hexdigest(),
        threads=tuple(threads),
    )
    with (output_dir / "capture.json").open("xb") as file:
        file.write(orjson.dumps(capture.model_dump(mode="json")))
    for provider in Provider:
        selected = [item for item in threads if item.thread.provider == provider]
        click.echo(
            f"{provider}: "
            + str(
                {
                    "canonical": sum(len(t.canonical) for t in selected),
                    "old_turns": sum(len(t.original_turns) for t in selected),
                    "bridged_turns": sum(len(t.bridged_turns) for t in selected),
                    "complete_turns": sum(len(t.turns) for t in selected),
                    "old_orphans": sum(
                        not turn.prompts for t in selected for turn in t.original_turns
                    ),
                    "new_orphans": sum(
                        not turn.prompts for t in selected for turn in t.turns
                    ),
                    "native_matched": sum(t.native_matched for t in selected),
                    "native_checked": sum(t.native_parent_checked for t in selected),
                    "native_mismatches": sum(
                        t.native_parent_mismatches for t in selected
                    ),
                    "compactions": sum(t.native_compactions for t in selected),
                }
            )
        )


if __name__ == "__main__":
    main()
