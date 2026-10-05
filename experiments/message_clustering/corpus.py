"""Freeze a read-only, private corpus before exposing clustering outputs."""

import hashlib
import os
import sqlite3
from enum import StrEnum
from pathlib import Path
from typing import Literal

import click
import orjson
from pydantic import BaseModel, ConfigDict, Field, ValidationError

from clustering import decode_stored_vector

SELECTION_SEED = "mash-message-clustering-20261005-v1"


class Provider(StrEnum):
    CLAUDE = "claude-code"
    CODEX = "codex"


class Split(StrEnum):
    DEVELOPMENT = "development"
    HELD_OUT = "held_out"


class Role(StrEnum):
    USER = "user"
    ASSISTANT = "assistant"


class Length(StrEnum):
    SHORT = "short"
    MEDIUM = "medium"
    LONG = "long"


class Record(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)


class DefaultBuild(Record):
    embedding_text: Literal["dialogue"]


class Message(Record):
    id: str
    ordinal: int = Field(ge=0)
    role: Role
    projection: str
    context_parent: int | None
    dimensions: int = Field(gt=0)
    norm: float = Field(gt=0, allow_inf_nan=False)
    vector_hex: str


class Thread(Record):
    id: str
    provider: Provider
    session: str | None
    source_path: Path
    title: str | None
    canonical_messages: int
    searchable_messages: int
    generation: str
    messages: tuple[Message, ...]

    @property
    def length(self) -> Length:
        if len(self.messages) <= 24:
            return Length.SHORT
        return Length.MEDIUM if len(self.messages) <= 150 else Length.LONG

    @property
    def projection_hashes(self) -> frozenset[str]:
        # Conservative leakage grouping, not a semantic/quality judgment.
        # Include all nonempty projections, including boilerplate; over-grouping
        # reduces available samples instead of leaking copies between splits.
        return frozenset(digest(message.projection) for message in self.messages)


class SelectedThread(Record):
    split: Split
    component: str
    thread: Thread


class Corpus(Record):
    schema_version: int = 1
    selection_seed: str = SELECTION_SEED
    gold_manifest: str
    source_threads: int
    ancestry_files_checked: int
    selected: tuple[SelectedThread, ...]


class CodexMeta(BaseModel):
    # Only lineage is needed; provider payloads contain unrelated instructions.
    model_config = ConfigDict(extra="ignore")
    id: str
    forked_from_id: str | None = None
    parent_thread_id: str | None = None


class CodexHeader(BaseModel):
    model_config = ConfigDict(extra="ignore")
    type: Literal["session_meta"]
    payload: CodexMeta


class ClaudeLineage(BaseModel):
    model_config = ConfigDict(extra="ignore")
    uuid: str | None = None
    parentUuid: str | None = None
    logicalParentUuid: str | None = None
    sessionId: str | None = None


def digest(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


def read_threads(database: Path) -> tuple[str, tuple[Thread, ...]]:
    """Take one SQLite read transaction, with no writes or source mutation."""
    with sqlite3.connect(f"{database.resolve().as_uri()}?mode=ro", uri=True) as db:
        db.row_factory = sqlite3.Row
        db.execute("BEGIN")
        manifest = db.execute("SELECT manifest FROM gold_manifest").fetchone()[0]
        # This corpus uses the existing default dialogue recipe. Fail closed
        # rather than silently deduplicating a title/parent/budget recipe wrong.
        build = db.execute(
            "SELECT json_extract(manifest, '$.build') FROM gold_manifest"
        ).fetchone()[0]
        DefaultBuild.model_validate_json(build)
        threads: list[Thread] = []
        rows = db.execute(
            """SELECT c.id,c.provider,c.source_session AS session,c.source_path,
                      c.title,count(m.id) AS canonical_messages,
                      sum(COALESCE(m.search_projection,m.content)<>'')
                          AS searchable_messages
               FROM conversations c JOIN messages m ON m.conversation_id=c.id
               WHERE c.provider IN ('claude-code','codex')
               GROUP BY c.id ORDER BY c.id"""
        ).fetchall()
        for row in rows:
            embeddings = db.execute(
                """SELECT m.id,m.ordinal,m.role,
                          COALESCE(m.search_projection,m.content) AS projection,
                          m.context_parent,e.generation,e.dimensions,e.norm,e.vector
                   FROM messages m JOIN message_embeddings e ON e.message_id=m.id
                   WHERE m.conversation_id=? ORDER BY m.ordinal,m.id""",
                (row["id"],),
            ).fetchall()
            if not embeddings:
                continue
            generations = {embedding["generation"] for embedding in embeddings}
            if len(generations) != 1:
                raise ValueError("thread mixes embedding generations")
            messages: list[Message] = []
            for embedding in embeddings:
                decode_stored_vector(
                    embedding["vector"], embedding["dimensions"], embedding["norm"]
                )
                fields = dict(embedding)
                del fields["generation"]
                fields["vector_hex"] = fields.pop("vector").hex()
                messages.append(Message.model_validate(fields))
            threads.append(
                Thread.model_validate(
                    dict(row) | {"generation": generations.pop(), "messages": messages}
                )
            )
    return manifest, tuple(threads)


def leakage_components(
    threads: tuple[Thread, ...],
) -> tuple[tuple[tuple[str, ...], ...], int]:
    """Group copies and known provider lineage before sampling/splitting."""
    parents = list(range(len(threads)))

    def root(index: int) -> int:
        while parents[index] != index:
            parents[index] = parents[parents[index]]
            index = parents[index]
        return index

    def join(left: int, right: int) -> None:
        a, b = root(left), root(right)
        parents[max(a, b)] = min(a, b)

    by_session: dict[tuple[Provider, str], int] = {}
    for index, thread in enumerate(threads):
        if thread.session:
            key = (thread.provider, thread.session)
            if key in by_session:
                join(index, by_session[key])
            by_session[key] = index
    # Shared missing parents must still join siblings whose ancestor isn't stored.
    lineage_owner: dict[tuple[Provider, str], int] = {}
    checked = 0
    for index, thread in enumerate(threads):
        links: set[str] = set()
        with thread.source_path.open() as source:
            if thread.provider == Provider.CODEX:
                meta = CodexHeader.model_validate_json(source.readline())
                links.add(meta.payload.id)
                links.update(
                    value
                    for value in (
                        meta.payload.forked_from_id,
                        meta.payload.parent_thread_id,
                    )
                    if value
                )
            else:
                for line in source:
                    if not line.strip():
                        continue
                    record = ClaudeLineage.model_validate_json(line)
                    links.update(
                        value
                        for value in (
                            record.uuid,
                            record.parentUuid,
                            record.logicalParentUuid,
                            record.sessionId,
                        )
                        if value
                    )
        checked += 1
        for link in links:
            key = (thread.provider, link)
            if key in lineage_owner:
                join(index, lineage_owner[key])
            lineage_owner[key] = index
    hashes = [thread.projection_hashes for thread in threads]
    for left, projections in enumerate(hashes):
        for right in range(left):
            common = len(projections & hashes[right])
            if common >= 3 and common >= 0.5 * min(
                len(projections), len(hashes[right])
            ):
                join(left, right)
    components: dict[int, list[str]] = {}
    for index, thread in enumerate(threads):
        components.setdefault(root(index), []).append(thread.id)
    component_by_id = {
        identity: tuple(sorted(identities))
        for identities in components.values()
        for identity in identities
    }
    return tuple(component_by_id[thread.id] for thread in threads), checked


def select_threads(
    threads: tuple[Thread, ...], components: tuple[tuple[str, ...], ...]
) -> tuple[SelectedThread, ...]:
    """Four independent components per provider/length: two in each split."""
    if len(threads) != len(components):
        raise ValueError("component count differs from thread count")
    used: set[tuple[str, ...]] = set()
    selected: list[SelectedThread] = []
    for provider in Provider:
        # Reserve independent >1000-vector cases before consuming other strata.
        for length in (Length.LONG, Length.MEDIUM, Length.SHORT):
            candidates = [
                (thread, component)
                for thread, component in zip(threads, components, strict=True)
                if thread.provider == provider
                and thread.length == length
                and len(thread.messages) >= 8
            ]
            candidates.sort(
                key=lambda item: (
                    0 if length == Length.LONG and len(item[0].messages) > 1000 else 1,
                    digest(SELECTION_SEED + item[0].id),
                )
            )
            count = 0
            for thread, component in candidates:
                if component in used:
                    continue
                used.add(component)
                selected.append(
                    SelectedThread(
                        split=Split.DEVELOPMENT if count % 2 == 0 else Split.HELD_OUT,
                        component=digest("\n".join(component)),
                        thread=thread,
                    )
                )
                count += 1
                if count == 4:
                    break
            if count != 4:
                raise ValueError(
                    "insufficient independent threads for a sampling stratum"
                )
    for split in Split:
        if not any(
            entry.split == split and len(entry.thread.messages) > 1000
            for entry in selected
        ):
            raise ValueError("independent long-thread coverage missing from split")
    return tuple(selected)


@click.command()
@click.option("--database", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(database: Path, output: Path) -> None:
    """Write private source data exclusively; refuse overwrite or repository output."""
    artifact_root = Path("/home/james/scratch/mash-artifacts").resolve()
    if not output.resolve().is_relative_to(artifact_root):
        raise click.ClickException("output must be in the private artifact directory")
    try:
        manifest, threads = read_threads(database)
        components, checked = leakage_components(threads)
        selected = select_threads(threads, components)
    except (OSError, sqlite3.Error, ValueError, ValidationError) as error:
        # Validation errors can include entire source records. Never print them
        # from a command whose inputs include private histories/instructions.
        raise click.ClickException(
            f"corpus freeze failed ({type(error).__name__}); no corpus written"
        ) from error
    corpus = Corpus(
        gold_manifest=manifest,
        source_threads=len(threads),
        ancestry_files_checked=checked,
        selected=selected,
    )
    encoded = orjson.dumps(corpus.model_dump(mode="json"))
    output.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    with os.fdopen(
        os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb"
    ) as file:
        file.write(encoded)
    click.echo(
        f"Frozen {len(selected)} threads; SHA-256 {hashlib.sha256(encoded).hexdigest()}"
    )


if __name__ == "__main__":
    main()
