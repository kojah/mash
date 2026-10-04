#!/usr/bin/env python3
"""Measure explicit shared gold rebuilds on copied certified silver.

Requires an otherwise empty disposable loopback Qdrant. Uses installed local
models, never provider files. Private captures must remain outside Git.
Timings include the HTTP API; this is not a fleet or semantic-quality evaluation.
"""

import argparse
import hashlib
import http.client
import importlib.util
import json
import os
import platform
import queue
import sqlite3
import subprocess
import threading
import urllib.parse
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "gold_cost", Path(__file__).with_name("benchmark-gold.py")
)
if SPEC is None or SPEC.loader is None:
    raise ImportError("benchmark-gold.py could not be loaded")
COST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COST)


def request(url, route, token=None):
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "http" or parsed.hostname != "127.0.0.1":
        raise ValueError("only disposable loopback HTTP endpoints are supported")
    client = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=120)
    try:
        client.request(
            "GET", route, headers={"Authorization": f"Bearer {token}"} if token else {}
        )
        response = client.getresponse()
        body = response.read()
        if response.status != 200:
            raise RuntimeError(f"{route}: HTTP {response.status}")
        return json.loads(body)
    finally:
        client.close()


def canonical(url, token):
    # This endpoint intentionally contains silver only, excluding gold vectors.
    return hashlib.sha256(
        json.dumps(
            request(url, "/v1/silver", token)["history"], sort_keys=True
        ).encode()
    ).hexdigest()


def backend_storage(url, root):
    collections = request(url, "/collections")["result"]["collections"]
    stats = {
        c["name"]: request(url, "/collections/" + c["name"])["result"]
        for c in collections
    }
    files = [p.stat() for p in root.rglob("*") if p.is_file()]
    return {
        "logical_file_bytes": sum(s.st_size for s in files),
        "allocated_file_bytes": sum(s.st_blocks * 512 for s in files),
        "collections": stats,
    }


class Server:
    def __init__(self, binary, config, output, number, activate):
        self.process = subprocess.Popen(
            [str(binary), "serve", "--server-config", str(config)]
            + (["--activate-gold"] if activate else []),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        lines = queue.Queue()
        # PIPE guarantees a stream; narrow once before the background reader owns it.
        stderr = self.process.stderr
        if stderr is None:
            raise RuntimeError("server stderr pipe was not created")

        def drain():
            with (output / f"server-{number}.jsonl").open("wb") as log:
                for line in stderr:
                    log.write(line)
                    log.flush()
                    lines.put(line)

        self.thread = threading.Thread(target=drain, daemon=True)
        self.thread.start()
        try:
            first = json.loads(lines.get(timeout=45))
            self.url = "http://" + first["listening"]
        except Exception:
            self.close()
            raise

    def close(self):
        self.process.terminate()
        try:
            self.process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.thread.join(timeout=5)


def configure(output, representation, group, url):
    node = output / "node.toml"
    node.write_text(
        'version=1\n[providers]\n[index]\ndestination="both"\n'
        + '[gold.query]\nintent="exact-mention"\n[shared]\n'
        + f'group_id={json.dumps(group)}\nnode_id="sample"\n'
        + f'api_url={json.dumps(url)}\ntoken_env="MASH_COST_TOKEN"\n'
    )
    server = output / "server.toml"
    server.write_text(
        'version=1\nlisten="127.0.0.1:0"\n'
        + 'qdrant_url_env="MASH_COST_QDRANT"\n'
        + f'[gold.build]\nrepresentations="{representation}"\n'
        + f"[[nodes]]\ngroup_id={json.dumps(group)}\n"
        + 'node_id="sample"\ntoken_env="MASH_COST_TOKEN"\n'
    )
    return node, server


def run(args):
    binary = args.binary.resolve(strict=True)
    source = args.source_db.resolve(strict=True)
    models = args.models.resolve(strict=True)
    storage = args.backend_storage.resolve(strict=True)
    output = args.output.resolve()
    if not 1 <= args.trials <= 20:
        raise ValueError("trials must be between 1 and 20")
    if output.is_relative_to(Path(__file__).resolve().parent.parent):
        raise ValueError("private output must be outside the repository")
    if request(args.qdrant_url, "/collections")["result"]["collections"]:
        raise ValueError("benchmark requires an empty disposable Qdrant instance")
    output.mkdir(mode=0o700)
    database = output / "index.sqlite3"
    with (
        sqlite3.connect(source.as_uri() + "?mode=ro", uri=True) as original,
        sqlite3.connect(database) as copied,
    ):
        original.backup(copied)
    local_silver = COST.silver_digest(database)
    os.environ["MASH_COST_QDRANT"] = args.qdrant_url
    os.environ["MASH_COST_TOKEN"] = os.urandom(32).hex()
    token = os.environ["MASH_COST_TOKEN"]
    group = "gold-cost-" + os.urandom(12).hex()
    stages = []
    for number, representation in enumerate(
        ("lexical-and-dense", "lexical-only", "lexical-and-dense")
    ):
        node, config = configure(output, representation, group, "")
        server = Server(binary, config, output, number, number != 0)
        try:
            configure(output, representation, group, server.url)
            command = [
                str(binary),
                "--db",
                str(database),
                "--models-dir",
                str(models),
                "--config",
                str(node),
            ]
            operation = ["index"] if number == 0 else ["index", "--rebuild-shared-gold"]
            result, timing = COST.measure(
                command + operation, binary, output / f"build-{number}"
            )
            if number == 0:
                if (
                    result["shared"]["state"] != "published"
                    or result["scanned_files"]
                    or result["silver"]["completeness"] != "complete"
                ):
                    raise ValueError(
                        "initial publication was incomplete or read provider files"
                    )
                shared_silver = canonical(server.url, token)
                source_state = request(server.url, "/v1/state", token)
            if COST.silver_digest(database) != local_silver:
                raise ValueError("publication/rebuild changed local canonical silver")
            if canonical(server.url, token) != shared_silver:
                raise ValueError("rebuild changed shared silver")
            state = request(server.url, "/v1/state", token)
            for key in (
                "revision",
                "silver_revision",
                "source_silver",
                "silver_scans",
                "sources",
                "tombstones",
            ):
                if state.get(key) != source_state.get(key):
                    raise ValueError("rebuild changed source identity: " + key)
            searches = []
            intents = ["exact-mention"] + (
                ["related-discussion"] if representation == "lexical-and-dense" else []
            )
            for intent in intents:
                for filtered in (False, True):
                    for query in COST.QUERIES:
                        for trial in range(args.trials):
                            response, measurement = COST.measure(
                                command
                                + [
                                    "search",
                                    query,
                                    "--shared",
                                    "--no-worker",
                                    "--intent",
                                    intent,
                                ]
                                + (
                                    ["--ranking", "fusion"]
                                    if intent == "related-discussion"
                                    else []
                                )
                                + (["--provider", "codex"] if filtered else []),
                                binary,
                                output / f"search-{number}-{len(searches)}",
                            )
                            searches.append(
                                {
                                    "intent": intent,
                                    "provider_filter": filtered,
                                    "query": query,
                                    "trial": trial,
                                    "measurement": measurement,
                                    "response": response,
                                }
                            )
            if request(server.url, "/v1/state", token) != state:
                raise ValueError("query-only operations changed shared state")
            if COST.silver_digest(database) != local_silver:
                raise ValueError("search changed local silver")
            stage = {
                "representation": representation,
                "operation": operation,
                "build": result,
                "measurement": timing,
                "searches": searches,
                "backend_storage": backend_storage(args.qdrant_url, storage),
            }
            stages.append(stage)
            COST.write_json(output / "stages.json", stages)
            print(
                json.dumps(
                    {
                        "stage": number,
                        "representation": representation,
                        "build_seconds": timing["wall_seconds"],
                    }
                ),
                flush=True,
            )
        finally:
            server.close()
    COST.write_json(
        output / "capture.json",
        {
            "scope": "copied-certified-silver-shared-representations-v1",
            "binary_sha256": COST.sha(binary),
            "script_sha256": COST.sha(Path(__file__)),
            "measurement_helper_sha256": COST.sha(
                Path(__file__).with_name("benchmark-gold.py")
            ),
            "model_marker_sha256": COST.sha(models / "installed.json"),
            "platform": platform.platform(),
            "cpu_count": os.cpu_count(),
            "qdrant_version": request(args.qdrant_url, "/"),
            "local_silver_sha256": local_silver,
            "shared_silver_sha256": shared_silver,
            "stages": stages,
            "limitations": [
                "single Linux node and loopback HTTP, 25-thread convenience sample",
                "CLI CPU/memory excludes API server, Qdrant and asynchronous optimizer work",
                "allocated backend snapshots include WAL and unreclaimed obsolete staging",
                "fresh CLI processes, filesystem/model pages may be cached",
                "20ms memory sampling gives lower bounds, wait4 may include launcher peaks",
                "no resident worker, concurrency, fleet, recall or semantic-quality claim",
            ],
        },
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "models", "source-db", "output", "backend-storage"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--qdrant-url", required=True)
    parser.add_argument("--trials", type=int, default=3)
    run(parser.parse_args())
