"""Private raw-float cache and bounded OpenRouter calls for encoder experiments."""

import hashlib
import re
import sqlite3
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from enum import StrEnum
from pathlib import Path
from typing import TYPE_CHECKING

import numpy as np
import orjson
from pydantic import ConfigDict

from clustering import VectorMatrix, normalized_vectors
from corpus import Record
from local_encoder import pool_vectors

if TYPE_CHECKING:
    from model_capacity import Capacity, TokenBudget


class Model(StrEnum):
    CODE = "voyageai/voyage-code-4"
    GENERAL = "voyageai/voyage-4"


class External(Record):
    model_config = ConfigDict(extra="ignore", frozen=True)


class Item(External):
    index: int
    embedding: tuple[float, ...]


class Usage(External):
    total_tokens: int
    cost: float | None = None


class Response(External):
    model: str
    data: tuple[Item, ...]
    usage: Usage
    id: str | None = None


class Routing(Record):
    allow_fallbacks: bool = False
    data_collection: str = "deny"


class Request(Record):
    model: Model
    input: tuple[str, ...]
    dimensions: int = 1024
    encoding_format: str = "float"
    provider: Routing = Routing()


class Receipt(Record):
    model: Model
    returned_model: str
    input_hashes: tuple[str, ...]
    tokens: int
    cost: float
    response_id: str | None
    seconds: float
    dimensions: int = 1024


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def decode(
    response: Response, count: int, model: Model, dimensions: int = 1024
) -> VectorMatrix:
    """Honor response indices rather than assuming response-array order."""
    if response.model not in (model.value, model.value.split("/", 1)[1]):
        raise ValueError("unexpected embedding response model")
    items = sorted(response.data, key=lambda item: item.index)
    if tuple(item.index for item in items) != tuple(range(count)):
        raise ValueError("missing or repeated embedding response index")
    vectors = np.array([item.embedding for item in items], dtype=np.float64)
    if vectors.shape != (count, dimensions) or not np.isfinite(vectors).all():
        raise ValueError("invalid raw embedding shape or values")
    normalized_vectors(vectors)  # Also reject zero vectors before caching.
    return vectors


def codec(vectors: VectorMatrix) -> VectorMatrix:
    return pool_vectors(
        vectors.astype(np.float32)[:, None, :],
        np.ones((len(vectors), 1), dtype=np.int64),
    )


def request(
    texts: tuple[str, ...], model: Model, key: str, dimensions: int = 1024
) -> tuple[VectorMatrix, Receipt]:
    started = time.perf_counter()
    operation = urllib.request.Request(
        "https://openrouter.ai/api/v1/embeddings",
        data=orjson.dumps(
            Request(model=model, input=texts, dimensions=dimensions).model_dump(
                mode="json"
            )
        ),
        headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"},
        method="POST",
    )
    for attempt in range(3):
        try:
            with urllib.request.urlopen(operation, timeout=60) as reply:
                response = Response.model_validate_json(reply.read())
            break
        except urllib.error.HTTPError as error:
            if error.code not in (429, 500, 502, 503, 504, 529) or attempt == 2:
                raise RuntimeError(f"embedding HTTP status {error.code}") from None
            time.sleep(2**attempt)
        except (TimeoutError, urllib.error.URLError):
            if attempt == 2:
                raise RuntimeError(
                    "embedding transport failure: charges unknown"
                ) from None
            time.sleep(2**attempt)
    vectors = decode(response, len(texts), model, dimensions)
    # Conservative fallback: both listed models are <=$0.12/M at registration.
    cost = response.usage.cost
    if cost is None:
        price = 0.06 if model == Model.GENERAL else 0.12
        cost = response.usage.total_tokens * price / 1_000_000
    if cost < 0 or not np.isfinite(cost) or response.usage.total_tokens < 0:
        raise ValueError("invalid API usage receipt")
    return vectors, Receipt(
        model=model,
        returned_model=response.model,
        input_hashes=tuple(digest(text.encode()) for text in texts),
        tokens=response.usage.total_tokens,
        cost=cost,
        response_id=response.id,
        seconds=time.perf_counter() - started,
        dimensions=dimensions,
    )


def structural_parts(text: str, limit: int = 28000) -> tuple[str, ...]:
    """Lossless byte-bounded pieces, preferring paragraph and line boundaries."""
    if limit < 4:
        raise ValueError("byte limit must fit a UTF-8 character")
    result = []
    current = ""
    for paragraph in re.split(r"(?<=\n)(?=\n)", text):
        segments = (
            (paragraph,)
            if len(paragraph.encode()) <= limit
            else tuple(paragraph.splitlines(keepends=True))
        )
        for segment in segments:
            while len(segment.encode()) > limit:
                if current:
                    result.append(current)
                    current = ""
                prefix = segment.encode()[:limit].decode(errors="ignore")
                result.append(prefix)
                segment = segment[len(prefix) :]
            if current and len((current + segment).encode()) > limit:
                result.append(current)
                current = ""
            current += segment
    if current:
        result.append(current)
    if "".join(result) != text:
        raise ValueError("structural splitting dropped source text")
    return tuple(result)


def batches(texts: tuple[str, ...]) -> tuple[tuple[str, ...], ...]:
    result = []
    current = []
    size = 0
    for text in texts:
        length = len(text.encode())
        if not text or length > 28000:
            raise ValueError("embedding input outside conservative context budget")
        if current and (len(current) == 64 or size + length > 100000):
            result.append(tuple(current))
            current, size = [], 0
        current.append(text)
        size += length
    if current:
        result.append(tuple(current))
    return tuple(result)


def build_cache(
    root: Path,
    texts: tuple[str, ...],
    model: Model,
    key_path: Path,
    budget: "TokenBudget | None" = None,
) -> None:
    """Resume only missing texts; commit buffered raw floats after each request pair."""
    capacity = budget.capacity if budget else None
    space = cache_space(model, capacity)
    dimensions = capacity.dimensions if capacity else 1024
    key = key_path.read_text().strip().split("=", 1)[1]
    receipts_path = root / "receipts.jsonl"
    receipts = tuple(
        Receipt.model_validate_json(line)
        for line in (
            receipts_path.read_bytes().splitlines() if receipts_path.exists() else ()
        )
    )
    spent = sum(item.cost for item in receipts)
    with sqlite3.connect(root / "raw-vectors.sqlite") as db:
        db.execute(
            "CREATE TABLE IF NOT EXISTS vectors "
            "(model TEXT, hash TEXT, vector BLOB, PRIMARY KEY(model,hash))"
        )
        present = {
            row[0]
            for row in db.execute("SELECT hash FROM vectors WHERE model=?", (space,))
        }
        missing = tuple(
            text
            for text in dict.fromkeys(texts)
            if digest(text.encode()) not in present
        )
        work = budget.batches(missing) if budget else batches(missing)
        with ThreadPoolExecutor(max_workers=2) as executor:
            for offset in range(0, len(work), 2):
                group = work[offset : offset + 2]
                estimated = (
                    sum(
                        budget.count(text) if budget else len(text.encode())
                        for batch in group
                        for text in batch
                    )
                    * 0.12
                    / 1_000_000
                )
                if spent + estimated > 3:
                    raise ValueError("registered $3 API charge cap reached")
                futures = [
                    executor.submit(request, batch, model, key, dimensions)
                    for batch in group
                ]
                for batch, future in zip(group, futures, strict=True):
                    vectors, receipt = future.result()
                    with receipts_path.open("ab") as stream:
                        stream.write(
                            orjson.dumps(receipt.model_dump(mode="json")) + b"\n"
                        )
                    spent += receipt.cost
                    with db:
                        db.executemany(
                            "INSERT OR IGNORE INTO vectors VALUES (?,?,?)",
                            (
                                (
                                    space,
                                    digest(text.encode()),
                                    vector.astype("<f8").tobytes(),
                                )
                                for text, vector in zip(batch, vectors, strict=True)
                            ),
                        )
                print(
                    f"{model}: batches {min(offset + 2, len(work))}/{len(work)} "
                    f"total cost ${spent:.4f}",
                    flush=True,
                )


def cache_space(model: Model, capacity: "Capacity | None" = None) -> str:
    if capacity is None:
        return model.value
    if capacity.model != model:
        raise ValueError("capacity belongs to another model")
    from model_capacity import vector_space

    return digest(orjson.dumps(vector_space(capacity).model_dump(mode="json")))


def read_cache(
    root: Path, model: Model, capacity: "Capacity | None" = None
) -> dict[str, VectorMatrix]:
    with sqlite3.connect(
        (root / "raw-vectors.sqlite").resolve().as_uri() + "?mode=ro", uri=True
    ) as db:
        result = {
            identity: np.frombuffer(blob, dtype="<f8").copy()[None, :]
            for identity, blob in db.execute(
                "SELECT hash,vector FROM vectors WHERE model=?",
                (cache_space(model, capacity),),
            )
        }
    dimensions = capacity.dimensions if capacity else 1024
    if any(
        v.shape != (1, dimensions) or not np.isfinite(v).all() for v in result.values()
    ):
        raise ValueError("cached vectors do not match model capabilities")
    return result
