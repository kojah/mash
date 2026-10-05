"""Private experimental gold storage with transactional publication and vector reuse."""

import hashlib
import sqlite3
from collections.abc import Callable
from pathlib import Path

import numpy as np
import orjson

from clustering import VectorMatrix
from corpus import Record
from local_encoder import Encoder
from turns import Turn, Unit


class Artifact(Record):
    source_sha256: str
    protocol_sha256: str
    encoder_generation: str
    turns: tuple[Turn, ...]
    units: tuple[Unit, ...]


def generation(encoder: Encoder) -> str:
    return hashlib.sha256(orjson.dumps(encoder.manifest.model_dump())).hexdigest()


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
        hashes = tuple(hashlib.sha256(text.encode()).hexdigest() for text in texts)
        found: dict[str, bytes] = {}
        missing: dict[str, str] = {}
        for digest, text in zip(hashes, texts, strict=True):
            row = self.db.execute(
                "SELECT vector FROM vectors WHERE generation=? AND text_hash=?",
                (fingerprint, digest),
            ).fetchone()
            if row:
                found[digest] = row[0]
            else:
                missing[digest] = text
        pending = tuple(missing.items())
        for start in range(0, len(pending), 8):
            batch = pending[start : start + 8]
            vectors = encoder.encode(tuple(text for _, text in batch))
            with self.db:
                for (digest, _), vector in zip(batch, vectors, strict=True):
                    blob = vector.astype("<f8").tobytes()
                    self.db.execute(
                        "INSERT INTO vectors VALUES (?,?,?)",
                        (fingerprint, digest, blob),
                    )
                    found[digest] = blob
            progress(min(start + 8, len(pending)), len(pending))
        matrix = (
            np.stack([np.frombuffer(found[digest], dtype="<f8") for digest in hashes])
            if hashes
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
