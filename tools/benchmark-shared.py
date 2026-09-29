#!/usr/bin/env python3
"""Exercise mash's authenticated HTTP API with deterministic synthetic vectors.

No model inference is included. Run against disposable benchmark groups only.
Each node token comes from MASH_BENCH_TOKEN_0, _1, and _2.
"""

import argparse
import concurrent.futures
import http.client
import json
import math
import os
import random
import statistics
import threading
import time
import urllib.parse

DIMENSIONS = 1024
GENERATION = "remote:voyage-4:1024:i8-cosine:v1"
WORDS = ("rust", "postgres", "redis", "compiler", "index", "search", "network", "history")


class Client:
    def __init__(self, url, node):
        parsed = urllib.parse.urlsplit(url)
        if parsed.scheme != "http":
            raise ValueError("benchmark expects HTTP on the private test network")
        self.connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=120)
        self.prefix = parsed.path.rstrip("/")
        self.token = os.environ[f"MASH_BENCH_TOKEN_{node}"]

    def call(self, route, payload=None, stream=False):
        headers = {"Authorization": f"Bearer {self.token}"}
        if payload is None:
            method, body = "GET", None
        else:
            method = "POST"
            body = payload if isinstance(payload, bytes) else encode(payload)
            headers["Content-Type"] = "application/x-ndjson" if stream else "application/json"
        start = time.perf_counter()
        self.connection.request(method, self.prefix + "/v1/" + route, body, headers)
        response = self.connection.getresponse()
        data = response.read()
        elapsed = time.perf_counter() - start
        if response.status != 200:
            raise RuntimeError(f"{route}: HTTP {response.status}: {data[:400]!r}")
        return elapsed, json.loads(data)

    def close(self):
        self.connection.close()


def encode(value):
    return json.dumps(value, separators=(",", ":"), allow_nan=False).encode()


def vector(node, ordinal):
    rng = random.Random(91031 + node * 1000000 + ordinal)
    return [rng.randint(-127, 127) for _ in range(DIMENSIONS)]


def records(node, start, end, revision):
    conversations, messages = [], []
    for offset in range(start // 32, math.ceil(end / 32)):
        conversations.append({"id": f"c-{offset}", "provider": "codex", "title": None,
                              "created_at": 1750000000, "updated_at": 1750000000 + revision})
    for ordinal in range(start, end):
        topic = WORDS[ordinal % len(WORDS)]
        text = (f"Discuss {topic} implementation and development on node {node}. "
                f"Session message {ordinal}; revision {revision}. "
                "Review code changes, debug errors and check concurrent database access. ") * 3
        messages.append({"id": f"m-{ordinal}", "conversation_id": f"c-{ordinal // 32}",
                         "ordinal": ordinal % 32, "role": "user", "content": text,
                         "projection": text, "created_at": 1750000000,
                         "generation": GENERATION, "vector": vector(node, ordinal)})
    return conversations, messages


def upload(node, size, revision, snapshot):
    conversations, messages = records(node, 0, size, revision)
    lines = [encode({"type": "start", "instance": f"bench-node-{node}",
                     "revision": revision, "base_revision": None if snapshot else revision - 1,
                     "snapshot": snapshot, "forgets": []})]
    for kind, rows in [("conversations", conversations), ("messages", messages)]:
        for offset in range(0, len(rows), 128):
            lines.append(encode({"type": "batch", "kind": kind,
                                 "records": rows[offset:offset + 128]}))
    lines.append(encode({"type": "finish"}))
    return b"\n".join(lines) + b"\n"


def summary(samples):
    values = sorted(sample * 1000 for sample in samples)
    if not values:
        return {"count": 0}
    return {"count": len(values), "mean_ms": statistics.mean(values),
            "p50_ms": statistics.median(values),
            "p95_ms": values[math.ceil(len(values) * .95) - 1],
            "p99_ms": values[math.ceil(len(values) * .99) - 1], "max_ms": values[-1]}


def load(args):
    results = []
    for node in range(3):
        client = Client(args.url, node)
        try:
            payload = upload(node, args.messages_per_node, 1, True)
            elapsed, result = client.call("index", payload, True)
            if result["counts"]["searchable_messages"] != args.messages_per_node:
                raise RuntimeError(f"incorrect loaded counts: {result}")
            results.append({"node": node, "bytes": len(payload), "seconds": elapsed,
                            "messages": args.messages_per_node})
        finally:
            client.close()
    return {"operation": "load", "url": args.url, "uploads": results}


def run(args):
    # Serialize requests outside the measured interval. Reuse one connection per worker.
    queries = [encode({"query": WORDS[i % len(WORDS)], "vector": vector(0, i),
                       "generation": GENERATION, "provider": None, "days": None})
               for i in range(64)]
    writes = []
    if args.writers:
        for node in range(args.writers):
            client = Client(args.url, node)
            _, state = client.call("state")
            client.close()
            revision = state["revision"]
            writes.append([upload(node, 64, revision + i + 1, False)
                           for i in range(args.updates)])
    started = []
    barrier = threading.Barrier(args.readers + args.writers,
                                action=lambda: started.append(time.perf_counter()))

    def reader(worker):
        client = Client(args.url, worker % 3)
        samples, errors = [], []
        try:
            for i in range(5):
                client.call("search", queries[i])
            barrier.wait(timeout=120)
            for i in range(args.requests):
                try:
                    elapsed, result = client.call("search", queries[(i + worker) % len(queries)])
                    if len(result["semantic"]) != 50:
                        raise RuntimeError("semantic retrieval returned fewer than 50 candidates")
                    # Each query exactly matches a node-0 fixture, independent of lexical ranking.
                    if result["semantic"][0]["semantic_score"] < .999:
                        raise RuntimeError("semantic top hit does not match its fixture vector")
                    samples.append(elapsed)
                except Exception as error:
                    errors.append(str(error))
        finally:
            client.close()
        return "read", samples, errors

    def writer(node):
        client = Client(args.url, node)
        samples, errors = [], []
        try:
            barrier.wait(timeout=120)
            for payload in writes[node]:
                try:
                    elapsed, result = client.call("index", payload, True)
                    if result["counts"]["searchable_messages"] != args.messages_per_node:
                        raise RuntimeError("incremental update changed fixture counts")
                    samples.append(elapsed)
                except Exception as error:
                    errors.append(str(error))
                    break
        finally:
            client.close()
        return "write", samples, errors

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.readers + args.writers) as executor:
        futures = [executor.submit(reader, i) for i in range(args.readers)]
        futures += [executor.submit(writer, i) for i in range(args.writers)]
        results = [future.result() for future in futures]
    elapsed = time.perf_counter() - started[0]
    reads = [sample for kind, samples, _ in results if kind == "read" for sample in samples]
    updates = [sample for kind, samples, _ in results if kind == "write" for sample in samples]
    errors = [error for _, _, failures in results for error in failures]
    return {"operation": "run", "url": args.url, "messages": 3 * args.messages_per_node,
            "dimensions": DIMENSIONS, "readers": args.readers, "writers": args.writers,
            "seconds": elapsed, "requests_per_second": len(reads) / elapsed,
            "search": summary(reads), "update_64_messages": summary(updates),
            "errors": errors, "read_samples_seconds": reads, "write_samples_seconds": updates}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["load", "run"])
    parser.add_argument("--url", required=True)
    parser.add_argument("--messages-per-node", type=int, default=1000)
    parser.add_argument("--readers", type=int, default=1)
    parser.add_argument("--requests", type=int, default=100)
    parser.add_argument("--writers", type=int, default=0)
    parser.add_argument("--updates", type=int, default=3)
    args = parser.parse_args()
    if not (1 <= args.readers <= 64 and 0 <= args.writers <= 3 and args.requests > 0
            and args.updates > 0 and args.messages_per_node >= 64):
        parser.error("invalid benchmark workload")
    result = load(args) if args.operation == "load" else run(args)
    print(json.dumps(result, allow_nan=False), flush=True)
    if result.get("errors"):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
