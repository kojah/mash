"""Read-only frozen, compatible user-vector controls, without text rewriting."""
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import sqlite3

import numpy as np
from numpy.typing import NDArray


@dataclass(frozen=True)
class Thread:
    id: str
    provider: str
    group: bytes
    messages: int
    text: str
    vector: NDArray[np.float64]


@dataclass(frozen=True)
class Dataset:
    generation: str
    fingerprint: str
    partition_sha256: str
    threads: tuple[Thread, ...]
    stored_user_vectors: int
    duplicate_messages: int
    duplicate_threads: int


def normalized(vector: NDArray[np.float64]) -> NDArray[np.float64]:
    norm = np.linalg.norm(vector)
    if not np.isfinite(vector).all() or not np.isfinite(norm) or norm <= 0:
        raise ValueError("invalid or cancelling topic vector")
    return vector / norm


def load(database: Path, partition_report: Path) -> Dataset:
    if not database.is_file():
        raise ValueError("frozen database is missing")
    wal = Path(str(database) + "-wal")
    if wal.exists() and wal.stat().st_size:
        raise ValueError("prepare a consistent frozen snapshot without a live WAL")
    if partition_report.stat().st_size > 64 * 1024 * 1024:
        raise ValueError("partition report exceeds the input cap")
    report_bytes = partition_report.read_bytes()
    report = json.loads(report_bytes)
    generation = report["generation"]
    if not isinstance(generation, str) or not generation:
        raise ValueError("invalid generation in partition report")
    groups = report["partition"]["source_ids"]
    if not isinstance(groups, dict):
        raise ValueError("invalid canonical partition")
    for identity in groups.values():
        if identity is not None and (
            not isinstance(identity, list)
            or len(identity) != 32
            or any(type(value) is not int or not 0 <= value <= 255 for value in identity)
        ):
            raise ValueError("invalid canonical source-group identity")
    digest = hashlib.sha256()

    def record(value: bytes) -> None:
        digest.update(len(value).to_bytes(8, "little"))
        digest.update(value)

    record(generation.encode())
    record(report_bytes)
    pending: dict[str, list[tuple[str, str, NDArray[np.float64]]]] = {}
    seen: set[tuple[str, str]] = set()
    duplicates = 0
    total = 0
    with sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("BEGIN")
        rows = connection.execute(
            "SELECT c.id,c.provider,m.id,m.content,COALESCE(m.search_projection,m.content),"
            "e.vector,e.generation FROM message_embeddings e JOIN messages m ON m.id=e.message_id "
            "JOIN conversations c ON c.id=m.conversation_id WHERE m.role='user' "
            "ORDER BY c.id,m.ordinal,m.id"
        )
        text_bytes = 0
        for thread, provider, message, canonical, text, raw, space in rows:
            total += 1
            if total > 100_000 or len(raw) != 384 or space != generation:
                raise ValueError("vector dimension, count or generation mismatch")
            if provider not in ("codex", "claude-code"):
                raise ValueError("initial experiment covers only the frozen two-provider cohort")
            if thread not in groups:
                raise ValueError("thread missing from frozen canonical partition")
            for value in (thread.encode(), provider.encode(), message.encode(), canonical.encode(), text.encode(), raw):
                record(value)
            text_bytes += len(text.encode())
            if text_bytes > 128 * 1024 * 1024:
                raise ValueError("text input cap exceeded")
            if not text.strip():
                continue
            key = (thread, text)
            if key in seen:
                duplicates += 1
                continue
            seen.add(key)
            vector = normalized(np.frombuffer(raw, dtype=np.int8).astype(np.float64))
            pending.setdefault(thread, []).append((provider, text, vector))
    threads = []
    used_groups = set()
    duplicate_threads = 0
    for thread, messages in sorted(pending.items()):
        if groups[thread] is None:
            raise ValueError("eligible thread lacks a canonical source group")
        group = bytes(groups[thread])
        if group in used_groups:
            duplicate_threads += 1
            continue
        used_groups.add(group)
        mean = normalized(np.mean([message[2] for message in messages], axis=0))
        threads.append(Thread(thread, messages[0][0], group, len(messages), "\n".join(message[1] for message in messages), mean))
    if not 2 <= len(threads) <= 8192:
        raise ValueError("invalid initial thread-mean cohort size")
    return Dataset(generation, digest.hexdigest(), hashlib.sha256(report_bytes).hexdigest(), tuple(threads), total, duplicates, duplicate_threads)
