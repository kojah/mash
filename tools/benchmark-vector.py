#!/usr/bin/env python3
"""Vector-only Redis FLAT/HNSW pilot; never point this at production Redis.

Uses existing document vectors with self-matches removed, not user-query
inference. No history text is exported. Artifacts contain sensitive embeddings.
"""

import argparse
import array
import hashlib
import json
import math
import random
import socket
import sqlite3
import struct
import time
import uuid
from pathlib import Path


class Redis:
    def __init__(self, port):
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=120)
        self.reader = self.socket.makefile("rb")

    def read(self):
        line = self.reader.readline()
        if not line.endswith(b"\r\n"):
            raise RuntimeError("truncated Redis reply")
        kind, value = line[:1], line[1:-2]
        if kind == b"-":
            raise RuntimeError(value.decode())
        if kind == b"+":
            return value
        if kind == b":":
            return int(value)
        if kind == b"$":
            length = int(value)
            if length == -1:
                return None
            payload = self.reader.read(length)
            if len(payload) != length or self.reader.read(2) != b"\r\n":
                raise RuntimeError("truncated Redis bulk reply")
            return payload
        if kind == b"*":
            return [self.read() for _ in range(int(value))]
        raise RuntimeError("unexpected Redis reply type")

    def batch(self, commands):
        output = bytearray()
        for command in commands:
            output.extend(f"*{len(command)}\r\n".encode())
            for argument in command:
                value = argument if isinstance(argument, bytes) else str(argument).encode()
                output.extend(f"${len(value)}\r\n".encode())
                output.extend(value)
                output.extend(b"\r\n")
        self.socket.sendall(output)
        return [self.read() for _ in commands]

    def call(self, *arguments):
        return self.batch([arguments])[0]

    def close(self):
        self.reader.close()
        self.socket.close()


def pairs(values):
    return {values[i].decode(): values[i + 1] for i in range(0, len(values), 2)}


def percentiles(values):
    values = sorted(values)
    return {name: round(values[max(0, math.ceil(len(values) * fraction) - 1)], 4)
            for name, fraction in [("p50", .5), ("p95", .95), ("p99", .99)]}


def freeze(source, destination):
    if destination.exists():
        raise ValueError("use a new artifact directory; refusing to overwrite a frozen corpus")
    with sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True) as original:
        original.execute("BEGIN")
        rows = original.execute("""SELECT e.vector, e.dimensions, e.generation,
            c.provider, coalesce(m.created_at, 0) FROM message_embeddings e
            JOIN messages m ON m.id=e.message_id
            JOIN conversations c ON c.id=m.conversation_id ORDER BY e.message_id""").fetchall()
    if not rows or len({(row[1], row[2]) for row in rows}) != 1:
        raise ValueError("corpus must have one nonempty embedding generation and dimension")
    with sqlite3.connect(destination) as frozen:
        frozen.execute("CREATE TABLE vectors(vector BLOB, dimensions INTEGER, generation TEXT, provider TEXT, created_at INTEGER)")
        frozen.executemany("INSERT INTO vectors VALUES(?,?,?,?,?)", rows)
    return rows


def query_arguments(vector, predicate, k, ef, policy=None, batch_size=None):
    # Request one extra neighbor and discard the source document after retrieval.
    # A numeric exclusion would turn even unfiltered queries into hybrid queries
    # and allow Redis's adaptive planner to select a brute-force filter strategy.
    options = ""
    if policy is not None:
        if not predicate or policy not in {"BATCHES", "ADHOC_BF"}:
            raise ValueError("hybrid policy requires a filter and a supported policy")
        options += f" HYBRID_POLICY {policy}"
    if batch_size is not None:
        if policy != "BATCHES" or batch_size < 1:
            raise ValueError("positive batch size requires BATCHES")
        options += f" BATCH_SIZE {batch_size}"
    expression = (f"({predicate})" if predicate else "*") + f"=>[KNN {k + 1} @vector $v{options} AS distance]"
    if ef is not None:
        expression += "=>{$EF_RUNTIME: " + str(ef) + "}"
    return (expression, "PARAMS", 2, "v", vector, "SORTBY", "distance",
            "RETURN", 1, "distance", "LIMIT", 0, k + 1, "DIALECT", 2)


def search(redis, index, vector, ordinal, predicate, k, ef=None, policy=None, batch_size=None):
    started = time.perf_counter()
    reply = redis.call("FT.SEARCH", index, *query_arguments(vector, predicate, k, ef, policy, batch_size))
    elapsed = (time.perf_counter() - started) * 1000
    found = {reply[i].decode(): float(pairs(reply[i + 1])["distance"])
             for i in range(1, len(reply), 2)
             if not reply[i].decode().endswith(f":{ordinal}")}
    found = dict(list(found.items())[:k])
    if len(found) > k or any(not math.isfinite(value) for value in found.values()):
        raise RuntimeError("invalid vector result")
    return elapsed, found


def json_value(value):
    if isinstance(value, bytes):
        return value.decode()
    if isinstance(value, list):
        return [json_value(item) for item in value]
    return value


def evaluate(redis, rows, size, queries, k, profiles=False, filtered_policies=False):
    prefix = "mash-vector-pilot:" + uuid.uuid4().hex + ":"
    dimensions = rows[0][1]
    documents = []
    for ordinal, row in enumerate(rows[:size]):
        values = array.array("b", row[0])
        if len(values) != dimensions or not any(values):
            raise ValueError("invalid quantized vector")
        vector = struct.pack("<" + "f" * dimensions, *values)
        documents.append((prefix + str(ordinal), vector, row[3], row[4]))
    indexes, report = [], []
    try:
        for start in range(0, size, 512):
            redis.batch([("HSET", key, "vector", vector, "provider", provider,
                          "time", created, "ordinal", start + offset)
                         for offset, (key, vector, provider, created) in enumerate(documents[start:start+512])])
        latest = sorted(row[3] for row in documents)[int((size - 1) * .99)]
        provider = min({row[2] for row in documents}, key=lambda p: sum(row[2] == p for row in documents))
        escaped_provider = "".join(c if c.isalnum() else "\\" + c for c in provider)
        filters = {"all": "", "provider": "@provider:{" + escaped_provider + "}",
                   "recent_1pct": f"@time:[{latest} +inf]"}
        thresholds = {"recent_1pct": latest}
        if filtered_policies:
            for fraction in [.5, .1]:
                name = f"recent_{round(fraction * 100)}pct"
                thresholds[name] = sorted(row[3] for row in documents)[int((size - 1) * (1 - fraction))]
                filters[name] = f"@time:[{thresholds[name]} +inf]"

        def matches(name, document):
            if name == "all":
                return True
            if name == "provider":
                return document[2] == provider
            return document[3] >= thresholds[name]

        selected = random.Random(731).sample(range(size), min(queries, size))
        ground_truth = {}
        for algorithm in ["FLAT", "HNSW"]:
            index = prefix + algorithm
            attributes = ["TYPE", "FLOAT32", "DIM", dimensions, "DISTANCE_METRIC", "COSINE"]
            if algorithm == "HNSW":
                attributes += ["M", 16, "EF_CONSTRUCTION", 200]
            started = time.perf_counter()
            redis.call("FT.CREATE", index, "ON", "HASH", "PREFIX", 1, prefix,
                       "SCHEMA", "provider", "TAG", "time", "NUMERIC", "ordinal", "NUMERIC",
                       "vector", "VECTOR", algorithm, len(attributes), *attributes)
            indexes.append(index)
            deadline = time.monotonic() + 300
            while True:
                info = pairs(redis.call("FT.INFO", index))
                if int(info["num_docs"]) == size and int(info["indexing"]) == 0:
                    break
                if time.monotonic() > deadline:
                    raise TimeoutError("index build did not complete")
                time.sleep(.05)
            build_ms = (time.perf_counter() - started) * 1000
            for ef in ([None] if algorithm == "FLAT" else [10, 50, 200]):
                for name, predicate in filters.items():
                    policies = [(None, None)]
                    if filtered_policies and algorithm == "HNSW" and predicate:
                        # Explicit brute force rejects EF_RUNTIME as irrelevant.
                        if ef == 10:
                            policies += [("ADHOC_BF", None)]
                        policies += [("BATCHES", None)]
                        policies += [("BATCHES", batch) for batch in [32, 128, 512]]
                    for policy, batch_size in policies:
                        report.append(measure(redis, index, documents, selected, name, predicate,
                            matches, ground_truth, algorithm,
                            None if policy == "ADHOC_BF" else ef, policy, batch_size, k,
                            size, build_ms, info, profiles))
        return report
    finally:
        for index in indexes:
            redis.call("FT.DROPINDEX", index)
        for start in range(0, len(documents), 512):
            redis.call("UNLINK", *[row[0] for row in documents[start:start+512]])


def measure(redis, index, documents, selected, name, predicate, matches, ground_truth,
            algorithm, ef, policy, batch_size, k, size, build_ms, info, profiles):
    timings, recalls, boundary_recalls = [], [], []
    per_query = []
    for ordinal in selected:
        vector = documents[ordinal][1]
        elapsed, found = search(redis, index, vector, ordinal, predicate, k, ef, policy, batch_size)
        for key in found:
            returned = int(key.rsplit(":", 1)[1])
            if not 0 <= returned < size or not matches(name, documents[returned]):
                raise RuntimeError("vector retrieval violated filter or corpus bounds")
        if algorithm == "FLAT":
            ground_truth[name, ordinal] = found
        truth = ground_truth[name, ordinal]
        if not truth or len(found) > len(truth):
            raise RuntimeError("invalid reference or result population")
        recalls.append(len(set(found) & set(truth)) / len(truth))
        boundary = max(truth.values()) + 1e-6
        boundary_recalls.append(sum(value <= boundary for value in found.values()) / len(truth))
        timings.append(elapsed)
        per_query.append(dict(query_ordinal=ordinal, latency_ms=round(elapsed, 4),
            returned=len(found), expected=len(truth), exact_id_recall=recalls[-1],
            boundary_recall=boundary_recalls[-1]))
    result = dict(size=size, algorithm=algorithm, ef_runtime=ef,
        hybrid_policy=policy or "AUTO", batch_size=batch_size,
        filter=name, filter_matches=sum(matches(name, row) for row in documents),
        queries=len(selected), k=k, build_ms=round(build_ms, 2),
        vector_index_mb=float(info["vector_index_sz_mb"]),
        latency_ms=percentiles(timings),
        recall_mean=round(sum(recalls)/len(recalls), 5), recall_min=min(recalls),
        boundary_recall_mean=round(sum(boundary_recalls)/len(boundary_recalls), 5),
        boundary_recall_min=min(boundary_recalls),
        incomplete_queries=sum(query["returned"] < query["expected"] for query in per_query),
        per_query=per_query)
    if profiles:
        profile = redis.call("FT.PROFILE", index, "SEARCH", "LIMITED", "QUERY",
            *query_arguments(documents[selected[0]][1], predicate, k, ef, policy, batch_size))
        result["execution_profile"] = json_value(profile[1])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument("--source", type=Path)
    inputs.add_argument("--frozen", type=Path)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--port", type=int, default=57379, choices=[57379])
    parser.add_argument("--sizes", type=int, nargs="+", default=[1000, 10000])
    parser.add_argument("--queries", type=int, default=100)
    parser.add_argument("--k", type=int, default=20)
    parser.add_argument("--profiles", action="store_true",
                        help="capture one FT.PROFILE per scenario after timing")
    parser.add_argument("--filtered-policies", action="store_true",
                        help="compare automatic, brute-force and batch policies across filter selectivities")
    args = parser.parse_args()
    args.artifacts.mkdir(parents=True, exist_ok=True, mode=0o700)
    if args.artifacts.stat().st_mode & 0o077:
        raise ValueError("artifact directory must be private (0700)")
    if (args.artifacts / "report.json").exists():
        raise ValueError("use a new report directory")
    corpus = args.frozen or args.artifacts / "vectors.sqlite3"
    if args.frozen:
        with sqlite3.connect(corpus.resolve().as_uri() + "?mode=ro", uri=True) as frozen:
            rows = frozen.execute("SELECT vector, dimensions, generation, provider, created_at FROM vectors ORDER BY rowid").fetchall()
    else:
        rows = freeze(args.source, corpus)
    if not rows or len({(row[1], row[2]) for row in rows}) != 1:
        raise ValueError("corpus must have one nonempty embedding generation and dimension")
    if args.queries < 1 or args.k < 1 or any(size <= args.k + 1 or size > len(rows) for size in args.sizes):
        raise ValueError("invalid query count, k or corpus size")
    redis = Redis(args.port)
    try:
        result = dict(corpus_sha256=hashlib.sha256(corpus.read_bytes()).hexdigest(),
                      vectors=len(rows), dimensions=rows[0][1], generation=rows[0][2],
                      redis_version=redis.call("INFO", "server").decode(),
                      host_load=Path("/proc/loadavg").read_text().strip(),
                      measurements=[])
        for size in args.sizes:
            result["measurements"].extend(evaluate(redis, rows, size, args.queries, args.k,
                                                 args.profiles, args.filtered_policies))
            (args.artifacts / "report.json").write_text(json.dumps(result, indent=2) + "\n")
            print(f"completed {size} vectors", flush=True)
    finally:
        redis.close()


if __name__ == "__main__":
    main()
