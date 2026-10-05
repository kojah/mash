"""Private experimental gold storage with transactional publication and vector reuse."""

import hashlib
import sqlite3
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import orjson

from clustering import VectorMatrix
from corpus import Record
from local_encoder import Encoder, Selection
from turns import Turn, Unit


class Artifact(Record):
    ancestry_sha256: str | None = None
    source_sha256: str
    protocol_sha256: str
    encoder_generation: str
    turns: tuple[Turn, ...]
    units: tuple[Unit, ...]


def generation(encoder: Encoder) -> str:
    return hashlib.sha256(orjson.dumps(encoder.manifest.model_dump())).hexdigest()


class SelectedText(Record):
    text: str
    selection: Selection | None = None


@dataclass(frozen=True)
class CacheRequest:
    generation: str
    digest: str
    input: SelectedText


class Store:
    """Cache batches durably; replace visible artifact only after a complete build."""

    def __init__(self, path: Path) -> None:
        self.db = sqlite3.connect(path)
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS vectors (
                generation TEXT NOT NULL, text_hash TEXT NOT NULL, vector BLOB NOT NULL,
                PRIMARY KEY (generation,text_hash));
            CREATE TABLE IF NOT EXISTS artifacts (
                name TEXT PRIMARY KEY, manifest BLOB NOT NULL);
        """)

    def close(self) -> None:
        self.db.close()

    def vectors(
        self,
        texts: tuple[str, ...],
        encoder: Encoder,
        progress: Callable[[int, int], None],
    ) -> tuple[VectorMatrix, int]:
        fingerprint = generation(encoder)
        requests = tuple(
            CacheRequest(
                fingerprint,
                hashlib.sha256(text.encode()).hexdigest(),
                SelectedText(text=text),
            )
            for text in texts
        )
        return self.cache_vectors(requests, encoder, progress)

    def selected_vectors(
        self,
        inputs: tuple[SelectedText, ...],
        encoder: Encoder,
        progress: Callable[[int, int], None],
    ) -> tuple[VectorMatrix, int]:
        """Reuse whole-input vectors; isolate target masks by namespace and span key."""
        fingerprint = generation(encoder)
        selected_generation = hashlib.sha256(
            (fingerprint + ":character-overlap-pooling-v1").encode()
        ).hexdigest()
        requests = tuple(
            CacheRequest(
                fingerprint if item.selection is None else selected_generation,
                hashlib.sha256(
                    item.text.encode()
                    if item.selection is None
                    else orjson.dumps(item.model_dump(mode="json"))
                ).hexdigest(),
                item,
            )
            for item in inputs
        )
        return self.cache_vectors(requests, encoder, progress)

    def cache_vectors(
        self,
        requests: tuple[CacheRequest, ...],
        encoder: Encoder,
        progress: Callable[[int, int], None],
    ) -> tuple[VectorMatrix, int]:
        found: dict[tuple[str, str], bytes] = {}
        missing: dict[tuple[str, str], CacheRequest] = {}
        for request in requests:
            key = (request.generation, request.digest)
            row = self.db.execute(
                "SELECT vector FROM vectors WHERE generation=? AND text_hash=?",
                key,
            ).fetchone()
            if row:
                found[key] = row[0]
            else:
                missing[key] = request
        pending = tuple(missing.items())
        for start in range(0, len(pending), 8):
            batch = pending[start : start + 8]
            texts = tuple(request.input.text for _, request in batch)
            selections = tuple(request.input.selection for _, request in batch)
            vectors = (
                encoder.encode_selected(texts, selections)
                if any(selection is not None for selection in selections)
                else encoder.encode(texts)
            )
            with self.db:
                for (key, _), vector in zip(batch, vectors, strict=True):
                    blob = vector.astype("<f8").tobytes()
                    self.db.execute(
                        "INSERT INTO vectors VALUES (?,?,?)",
                        (*key, blob),
                    )
                    found[key] = blob
            progress(min(start + 8, len(pending)), len(pending))
        matrix = (
            np.stack(
                [
                    np.frombuffer(
                        found[(request.generation, request.digest)], dtype="<f8"
                    )
                    for request in requests
                ]
            )
            if requests
            else np.empty((0, 384))
        )
        return matrix, len(pending)

    def publish(self, name: str, artifact: Artifact) -> None:
        """Atomically replace visible units; vector cache remains reusable."""
        with self.db:
            self.db.execute(
                "INSERT OR REPLACE INTO artifacts VALUES (?,?)",
                (name, orjson.dumps(artifact.model_dump(mode="json"))),
            )

    def read(self, name: str) -> Artifact | None:
        row = self.db.execute(
            "SELECT manifest FROM artifacts WHERE name=?", (name,)
        ).fetchone()
        return Artifact.model_validate_json(row[0]) if row else None
