"""Label-only field-role controls with frozen previously discovered memberships."""
import argparse
from collections import Counter, defaultdict
from dataclasses import asdict
from enum import StrEnum
import importlib.metadata
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer

from feedback_fields import TASK, Role, Selection, included, parse, records
from feedback_payload import Status, select
from packet_review import validate
from pair_review import Key
from phrase_labels import source_terms, supported
from reference_review import digest
from subject_labels import label_scores, ranked_terms


class Score(StrEnum):
    CTFIDF = "ctfidf"
    IDF = "record-idf"
    DIVERSE = "record-idf-containment-diversity"


def memory():
    """Keep independent Linux process-image HWM alongside OS rusage evidence."""
    usage = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024 if platform.system() == "Darwin" else 1024)
    status = Path('/proc/self/status')
    hwm = None
    if platform.system() == 'Linux' and status.exists():
        values = [line.split() for line in status.read_text().splitlines() if line.startswith('VmHWM:')]
        if len(values) != 1 or len(values[0]) != 3 or values[0][2] != 'kB':
            raise ValueError('unexpected Linux process-image memory measurement')
        hwm = int(values[0][1]) / 1024
    return {'peak_rss_mib': usage, 'linux_vm_hwm_mib': hwm}


def load_scope(packet_bytes, review_bytes, capture_bytes):
    validate(packet_bytes, review_bytes)
    packet, capture = json.loads(packet_bytes), json.loads(capture_bytes)
    if (capture.get("schema_version") != 1 or capture.get("scope") != "development-explicit-feedback-payload-pilot"
            or capture["input_sha256"]["packets"] != digest(packet_bytes)
            or capture["input_sha256"]["review"] != digest(review_bytes)
            or capture["dataset_fingerprint"] != packet["input_fingerprint"]):
        raise ValueError("frozen membership source provenance changed")
    modes = [c for c in capture["captures"] if c["mode"] == "explicit-feedback-payload"]
    if len(modes) != 1:
        raise ValueError("one exact payload capture required")
    fitted = [p for p in modes[0]["reports"] if p["method"] == "grinch" and p["reverse"] is False]
    if len(fitted) != 1:
        raise ValueError("one frozen forward GRINCH membership required")
    sources = {(c["thread"], t["ordinal"]): t for c in packet["cases"] for t in c["turns"]}
    units = {}
    for entry in modes[0]["coverage"]:
        if not entry["encoded"]:
            continue
        key = Key(**entry["key"])
        source = sources[(key.thread, key.ordinal)]
        text = source["text"]
        bounds = select(text)
        if (bounds.status != Status.PAYLOAD or (entry["start"], entry["end"]) != (bounds.start, bounds.end)
                or source["id"] != entry["source_id"] or source["text_sha256"] != entry["source_sha256"]
                or digest(text[bounds.start:bounds.end].encode()) != entry["text_sha256"]):
            raise ValueError("frozen payload identity/bounds changed")
        if key in units:
            raise ValueError("duplicate source payload")
        units[key] = text[bounds.start:bounds.end]
    clusters = fitted[0]["clusters"]
    groups = [tuple(Key(**k) for k in c["members"]) for c in clusters]
    members = [key for group in groups for key in group]
    if not groups or any(not group for group in groups) or len(set(members)) != len(members) or set(members) != set(units):
        raise ValueError("fixed label scope must exactly partition captured source units")
    return capture["dataset_fingerprint"], units, groups, [c["keywords"] for c in clusters]


def label(units: dict, groups: list[tuple], parsed: dict, selection: Selection) -> dict:
    keys = sorted(units)
    positions = {key: i for i, key in enumerate(keys)}
    selected = {key: records(units[key], parsed[key], selection) for key in keys}
    parts = [(key, text) for key in keys for text in selected[key] if source_terms(text)]
    if not parts:
        return {"vocabulary": 0, "clusters": [], "reason": "no eligible label terms"}
    vectorizer = CountVectorizer(analyzer=source_terms, max_features=8192)
    span_counts = vectorizer.fit_transform(text for _, text in parts).toarray()
    background = np.zeros((len(keys), span_counts.shape[1]), dtype=np.int64)
    for (key, _), counts in zip(parts, span_counts):
        background[positions[key]] += counts
    counts = np.stack([background[[positions[k] for k in group]].sum(axis=0) for group in groups])
    baseline, idf = label_scores(counts, background)
    terms = vectorizer.get_feature_names_out().tolist()
    result = []
    for i, group in enumerate(groups):
        role_terms = defaultdict(Counter)
        for key in group:
            for span in parsed[key]:
                role_terms[span.role].update(source_terms(units[key][span.start:span.end]))
        methods = {}
        for score in Score:
            names = ranked_terms(baseline[i] if score == Score.CTFIDF else idf[i], terms,
                                 diverse=score == Score.DIVERSE)
            if any(not supported(name["term"], [p for key in group for p in selected[key]]) for name in names):
                raise ValueError("label is not an exact retained source phrase")
            for name in names:
                roles = sorted(role for role, values in role_terms.items() if values[name["term"]] > 0)
                name["source_roles"] = roles
                name["metadata_framing_only"] = bool(roles) and not bool(set(roles) & TASK)
                name["no_single_role_support"] = not bool(roles)
            methods[score] = names
        result.append({"members": [asdict(k) for k in group], "labels": methods})
    return {"vocabulary": len(terms), "vocabulary_sha256": digest(json.dumps(terms).encode()),
            "record_term_counts": [int(row.sum()) for row in background], "clusters": result}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("packets", "review", "memberships", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]) or args.output.exists():
        raise ValueError("use a fresh private output path")
    started, cpu_started = time.perf_counter(), time.process_time()
    raw = [p.read_bytes() for p in (args.packets, args.review, args.memberships)]
    if max(map(len, raw)) > 16 * 1024 * 1024:
        raise ValueError("label input exceeds cap")
    fingerprint, units, groups, expected = load_scope(*raw)
    parse_start = time.perf_counter()
    parsed = {key: parse(text) for key, text in units.items()}
    parsing_ms = (time.perf_counter() - parse_start) * 1000
    captures = []
    for selection in Selection:
        label_start = time.perf_counter()
        result = label(units, groups, parsed, selection)
        label_ms = (time.perf_counter() - label_start) * 1000
        if selection == Selection.ALL:
            actual = [[{k: v for k, v in name.items() if k in ("term", "score")} for name in c["labels"][Score.CTFIDF]]
                      for c in result["clusters"]]
            if actual != expected:
                raise ValueError("full-payload baseline does not reproduce frozen labels exactly")
        coverage = [{"key": asdict(key), "source_chars": len(units[key]),
                     "selected_chars": sum(s.end - s.start for s in parsed[key] if included(s.role, selection)),
                     "selected_span_count": len(records(units[key], parsed[key], selection)),
                     "retained_roles": sorted({s.role for s in parsed[key] if included(s.role, selection)})} for key in sorted(units)]
        for row, key in zip(coverage, sorted(units)):
            targets = {term for s in parsed[key] if s.role == Role.TARGET
                       for term in source_terms(units[key][s.start:s.end])}
            retained = {term for part in records(units[key], parsed[key], selection) for term in source_terms(part)}
            row['lost_target_terms'] = sorted(targets - retained)
        captures.append({"selection": selection, "coverage": coverage, "label_ms": label_ms, **result})
    report = {"schema_version": 1, "scope": "development-fixed-membership-feedback-field-labels",
              "dataset_fingerprint": fingerprint, "input_sha256": dict(zip(("packets", "review", "memberships"), map(digest, raw))),
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                 ("field_labels.py", "feedback_fields.py", "feedback_payload.py", "packet_review.py", "source_spans.py", "phrase_labels.py",
                  "subject_labels.py", "reference_review.py", "episodes.py", "data.py", "uv.lock", "lexical_boundaries.py")},
              "python": platform.python_version(), "versions": {name: importlib.metadata.version(name) for name in ("numpy", "scikit-learn")},
              "records": len(units), "groups": len(groups), "captures": captures,
              "membership_sha256": digest(json.dumps([[asdict(k) for k in g] for g in groups], sort_keys=True).encode()),
              "field_spans": [{"key": asdict(k), "payload_sha256": digest(units[k].encode()), "spans": [asdict(s) for s in parsed[k]]} for k in sorted(units)],
              "parameters": {"scorers": list(Score), "selections": list(Selection), "limit": 8, "max_features": 8192,
                             "features": "source-adjacent unigrams/bigrams per exact span, count separately, record-level DF",
                             "diversity": "lexical atom containment, not semantic MMR", "membership": "frozen forward GRINCH from explicit payload pilot"},
              "inference_calls": 0, "parsing_ms": parsing_ms,
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "cpu_ms": (time.process_time() - cpu_started) * 1000,
              **memory(),
              "limitations": ["one exposed known schema/thread, oracle reviewed task/window scope, fixed imperfect memberships",
                              "field roles identify syntax, not independently validated semantic importance or labels",
                              "feedback-only drops target text that may resolve referents; unknown content retained, unsupported layouts fail closed",
                              "field-specific counts can remove cross-field or cross-paragraph bigrams; all names audited for retained source support",
                              "record DF/vocabulary are transductive on eight source records, not independent corpus statistics or online maintenance",
                              "metadata-only term support audit is a role diagnostic, not topic accuracy/parent correctness",
                              "no discovery, context resolution, full ingestion gate, Rust promotion or guided-search gain established"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"records": len(units), "groups": len(groups), "controls": len(captures) * len(Score),
                      "baseline_reproduced": True, "inference_calls": 0, "parsing_ms": parsing_ms}))


if __name__ == "__main__":
    main()
