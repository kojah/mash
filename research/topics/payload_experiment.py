"""Exposed exact feedback-envelope pilot; oracle task scope, no semantic rewriting."""
import argparse
from collections import Counter
from dataclasses import asdict
import importlib.metadata
import itertools
import json
from pathlib import Path
import platform
import resource
import sqlite3
import time

from data import load
from episodes import read_turns, source_hash
from feedback_payload import Status, select
from local_embeddings import Encoder
from packet_review import Origin, validate
from pair_review import Key, Pair, Relation
from preceding_context import Role
from reference_review import digest
from source_spans import Kind
from span_units import Unit
from unit_grouping import Method, Representation, cluster, evaluate, keywords, points


def source_scope(args, packet_bytes, review_bytes):
    judgments = validate(packet_bytes, review_bytes)
    packet = json.loads(packet_bytes)
    dataset = load(args.database, args.partition_report)
    if (packet["generation"] != dataset.generation or packet["input_fingerprint"] != dataset.fingerprint
            or packet["partition_sha256"] != dataset.partition_sha256):
        raise ValueError("source-review dataset changed")
    eligible = {t.id: t for t in dataset.threads}
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("BEGIN")
        for case in packet["cases"]:
            thread = eligible.get(case["thread"])
            if thread is None or thread.group[0] % 5 == 0 or thread.group.hex() != case["source_group"]:
                raise ValueError("packet must use exact development canonical group")
            turns = read_turns(connection, thread.id, dataset.generation)
            if source_hash(turns) != case["source_sha256"] or len(turns) != len(case["turns"]):
                raise ValueError("packet complete source sequence changed")
            for turn, recorded in zip(turns, case["turns"]):
                if (turn.id, turn.ordinal, digest(turn.text.encode())) != (recorded["id"], recorded["ordinal"], recorded["text_sha256"]):
                    raise ValueError("packet native identity changed")
    sources = {(c["thread"], t["ordinal"]): t for c in packet["cases"] for t in c["turns"]}
    selected = [j for j in judgments if j.facets]
    if not selected or any(j.origin != Origin.WRAPPED for j in selected):
        raise ValueError("intact wrapped task facet scope required")
    pairs = tuple(Pair(Key(a.thread, a.ordinal, 0), Key(b.thread, b.ordinal, 0),
                       Relation.OVERLAP if a.facets & b.facets else Relation.DISTINCT)
                  for a, b in itertools.combinations(selected, 2))
    return dataset, judgments, sources, selected, pairs


def matched_sources(sources, selected, tokenizer):
    """Use exactly the same model-window support in both representations."""
    matched = set()
    for judgment in selected:
        source = sources[(judgment.thread, judgment.ordinal)]
        selection = select(source["text"])
        if selection.status != Status.PAYLOAD:
            raise ValueError("reviewed task lacks one unambiguous feedback envelope")
        texts = (source["text"], source["text"][selection.start:selection.end])
        if all(2 <= len(tokenizer.encode(text).ids) <= 512 for text in texts):
            matched.add((judgment.thread, judgment.ordinal))
    return matched


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "packets", "review", "snapshot", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]) or args.output.exists():
        raise ValueError("use a fresh private output path")
    started, cpu_started = time.perf_counter(), time.process_time()
    packet_bytes, review_bytes = args.packets.read_bytes(), args.review.read_bytes()
    dataset, judgments, sources, selected, pairs = source_scope(args, packet_bytes, review_bytes)
    encoder = Encoder(args.snapshot)
    matched = matched_sources(sources, selected, encoder.tokenizer)
    captures = []
    for mode in ("intact", "explicit-feedback-payload"):
        before = asdict(encoder.statistics)
        selection_start = time.perf_counter()
        units, coverage = [], []
        for judgment in selected:
            source = sources[(judgment.thread, judgment.ordinal)]
            text = source["text"]
            selection = select(text)
            if selection.status != Status.PAYLOAD:
                raise ValueError("reviewed task lacks one unambiguous feedback envelope")
            start, end = (0, len(text)) if mode == "intact" else (selection.start, selection.end)
            units.append(Unit(Key(judgment.thread, judgment.ordinal, 0), Role.USER, Kind.PROSE,
                              start, end, source["id"], source["text_sha256"], text[start:end]))
        selection_ms = (time.perf_counter() - selection_start) * 1000
        encode_start = time.perf_counter()
        vectors = {}
        for unit in units:
            tokens = len(encoder.tokenizer.encode(unit.text).ids)
            encoded = (unit.key.thread, unit.key.ordinal) in matched
            vectors[unit.key] = encoder.encode(unit.text) if encoded else None
            coverage.append({**unit.metadata(), "tokens": tokens, "encoded": encoded,
                             "source_bytes": len(sources[(unit.key.thread, unit.key.ordinal)]["text"].encode()),
                             "selected_bytes": len(unit.text.encode())})
        encoding_ms = (time.perf_counter() - encode_start) * 1000
        after = asdict(encoder.statistics)
        represented = points(tuple(units), vectors, Representation.SPAN)
        reports = []
        for method in Method:
            for reverse in (False, True):
                labels, grouping_ms = cluster(represented, method, reverse)
                label_start = time.perf_counter()
                names = keywords(tuple(units), represented, labels)
                labeling_ms = (time.perf_counter() - label_start) * 1000
                evaluation_start = time.perf_counter()
                assessment = evaluate(pairs, represented, labels)
                reports.append({"method": method, "reverse": reverse, "points": len(represented),
                                "cluster_count": len(names), "noise_points": int((labels < 0).sum()),
                                "clusters": names, "grouping_ms": grouping_ms, "labeling_ms": labeling_ms,
                                "evaluation_ms": (time.perf_counter() - evaluation_start) * 1000, **assessment})
        captures.append({"mode": mode, "coverage": coverage, "selection_ms": selection_ms,
                         "encoding_ms": encoding_ms, "encoder_delta": {k: after[k] - before[k] for k in
                            ("calls", "cache_hits", "inference_ms", "tokenization_ms")}, "reports": reports})
    report = {"schema_version": 1, "scope": "development-explicit-feedback-payload-pilot",
              "dataset_fingerprint": dataset.fingerprint,
              "input_sha256": {"packets": digest(packet_bytes), "review": digest(review_bytes), "partition_report": dataset.partition_sha256},
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                 ("payload_experiment.py", "feedback_payload.py", "packet_review.py", "data.py", "episodes.py", "uv.lock",
                  "local_embeddings.py", "unit_grouping.py", "online_tree.py", "phrase_labels.py", "subject_labels.py",
                  "lexical_boundaries.py", "reference_review.py", "pair_review.py", "preceding_context.py", "source_spans.py", "span_units.py")},
              "python": platform.python_version(), "platform": platform.platform(),
              "versions": {name: importlib.metadata.version(name) for name in ("numpy", "scikit-learn")},
              "origin_counts": dict(Counter(j.origin for j in judgments)), "reviewed_records": len(judgments),
              "facet_records": len(selected), "facet_threads": len({j.thread for j in selected}),
              "matched_records": len(matched),
              "pairs": dict(Counter(p.relation for p in pairs)), "captures": captures,
              "encoder": asdict(encoder.statistics),
              "parameters": {"envelope": "one exact submitted_feedback opening/closing pair, nonempty payload, otherwise fail closed",
                             "clustering": "unchanged BIRCH.4/branch64/no global; GRINCH all-mergers.7; average cosine distance.3; HDBSCAN min5/EOM",
                             "labels": "source-adjacent c-TF-IDF, top8/max8192"},
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "cpu_ms": (time.process_time() - cpu_started) * 1000,
              "peak_rss_mib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024 if platform.system() == "Darwin" else 1024),
              "limitations": ["oracle facet/task admission on9 records in one thread, not automatic origin classifier",
                              "one known explicit envelope schema, exposed after source inspection, not general context selection",
                              "coding-assistant review not independent/human/held-out; other57 records have origin judgments only",
                              "facet names/rationales never enter inference or discovery; no endorsement inferred",
                              "whole payload retains page metadata/element paths; no generative rewrite or field cleaning",
                              "both representations restricted to the same model-window intersection; incompatible records/pairs abstain without truncation",
                              "shared encoder cache and tiny grouping sample; no full-ingestion performance or topic quality gate",
                              "no default/Rust promotion, semantic ancestors or guided-search gain established"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"records": len(judgments), "facet_records": len(selected), "pairs": report["pairs"],
                      "controls": sum(len(c["reports"]) for c in captures), "encoder": report["encoder"]}))


if __name__ == "__main__":
    main()
