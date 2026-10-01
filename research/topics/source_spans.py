"""Exact source-offset structural units; no rewriting, topic assignment or inference."""
import argparse
from dataclasses import asdict, dataclass
from enum import StrEnum
import json
from pathlib import Path
import re
import time

from episodes import read_cases
from lexical_boundaries import admissions
from reference_review import digest, validate


class Kind(StrEnum):
    PROSE = "paragraph"
    LIST = "list-item"
    TABLE = "table-row"
    HEADING = "heading"
    CODE = "code-block"


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    kind: Kind


LIST = re.compile(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)")
HEADING = re.compile(r"^\s*#{1,6}\s+")


def table_separator(line: str) -> bool:
    cells = line.strip().strip("|").split("|")
    return len(cells) >= 2 and all(re.fullmatch(r"\s*:?-{3,}:?\s*", cell) for cell in cells)


def structural_spans(text: str) -> tuple[Span, ...]:
    """Trim only boundary whitespace. Offsets are Python Unicode code points."""
    lines = text.splitlines(keepends=True)
    offsets, cursor = [], 0
    for line in lines:
        offsets.append(cursor)
        cursor += len(line)
    result, pending, table = [], None, False

    def emit(start: int, end: int, kind: Kind) -> None:
        while start < end and text[start].isspace():
            start += 1
        while end > start and text[end - 1].isspace():
            end -= 1
        if end > start:
            result.append(Span(start, end, kind))

    def flush(end: int) -> None:
        nonlocal pending
        if pending is not None:
            emit(pending, end, Kind.PROSE)
            pending = None

    fenced, fence_character, fence_length, fence_start = False, "", 0, None
    for i, line in enumerate(lines):
        start, end = offsets[i], offsets[i] + len(line)
        fence = re.match(r"^\s*(`{3,}|~{3,})", line)
        if fenced:
            if fence and fence.group(1)[0] == fence_character and len(fence.group(1)) >= fence_length:
                emit(fence_start, end, Kind.CODE)
                fenced, fence_start = False, None
            continue
        if fence:
            flush(start)
            fenced, fence_start = True, start
            fence_character, fence_length = fence.group(1)[0], len(fence.group(1))
            table = False
            continue
        if not line.strip():
            flush(start)
            table = False
            continue
        if i + 1 < len(lines) and "|" in line and table_separator(lines[i + 1]):
            flush(start)
            table = True
            continue  # Header is framing, not a topic candidate.
        if table and table_separator(line):
            continue
        if table and "|" in line:
            flush(start)
            emit(start, end, Kind.TABLE)
        else:
            table = False
            if LIST.match(line) or HEADING.match(line):
                flush(start)
                emit(start, end, Kind.LIST if LIST.match(line) else Kind.HEADING)
            elif pending is None:
                pending = start
    if fenced:
        emit(fence_start, len(text), Kind.CODE)
    flush(len(text))
    if any(a.end > b.start for a, b in zip(result, result[1:])):
        raise ValueError("overlapping source spans")
    return tuple(result)


def load_documents(database: Path, partition: Path, annotations: Path, source_review: Path,
                   contexts_path: Path, reference_review: Path) -> tuple[str, dict]:
    """Shared frozen source boundary for captures and unit-grouping experiments."""
    fingerprint, cases = read_cases(database, partition, annotations)
    admitted = admissions(cases, source_review)
    context_bytes, review_bytes, reference_bytes = (p.read_bytes() for p in (contexts_path, source_review, reference_review))
    judgments = validate(context_bytes, review_bytes, json.loads(reference_bytes))
    context_report = json.loads(context_bytes)
    if context_report["dataset_fingerprint"] != fingerprint:
        raise ValueError("context dataset changed")
    contexts = {(c["thread"], x["anchor"]["ordinal"]): x for c in context_report["cases"] for x in c["contexts"]}
    documents = {}
    for case in cases:
        for turn in case.turns:
            if turn.id not in admitted:
                continue
            identity = contexts[(case.thread, turn.ordinal)]["anchor"]
            if identity["id"] != turn.id or identity["searchable_sha256"] != digest(turn.text.encode()):
                raise ValueError("source anchor changed")
            documents[(case.thread, turn.ordinal)] = (identity, turn.text)
    for judgment in judgments:
        context = contexts[(judgment.thread, judgment.ordinal)]
        for ordinal in judgment.evidence_ordinals:
            entry = next(e for e in context["display"] if e["ordinal"] == ordinal)
            identity = {k: entry[k] for k in context["anchor"]}
            key = (judgment.thread, ordinal)
            value = (identity, entry["text"])
            if key in documents and documents[key] != value:
                raise ValueError("conflicting source identity")
            documents[key] = value
    return fingerprint, documents


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "contexts", "reference-review", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private source-span reports must remain outside Git")
    started = time.perf_counter()
    fingerprint, documents = load_documents(args.database, args.partition_report, args.annotations,
                                           args.source_review, args.contexts, args.reference_review)
    start = time.perf_counter()
    rows = []
    for (thread, ordinal), (identity, text) in sorted(documents.items()):
        spans = structural_spans(text)
        rows.append({"thread": thread, "identity": identity,
                     "source_chars": len(text), "source_bytes": len(text.encode()),
                     "spans": [{**asdict(s), "text_sha256": digest(text[s.start:s.end].encode()),
                                "source_bytes": len(text[s.start:s.end].encode())} for s in spans]})
    report = {"schema_version": 1, "scope": "development-admitted-anchors-and-oracle-reference-evidence-spans",
              "dataset_fingerprint": fingerprint,
              "input_sha256": {name: digest(getattr(args, name).read_bytes()) for name in ("annotations", "source_review", "contexts", "reference_review")},
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                                 ("source_spans.py", "episodes.py", "lexical_boundaries.py", "reference_review.py", "data.py", "uv.lock")},
              "rows": rows,
              "offset_unit": "unicode-code-points", "segmentation_ms": (time.perf_counter() - start) * 1000,
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000, "inference_calls": 0,
              "limitations": ["structural units are not semantic topics or endorsed source spans",
                              "oracle admitted anchors/evidence scope, not automatic context selection",
                              "table headers/separators and whitespace are excluded; headings remain separate candidates",
                              "list continuations can remain separate prose; inline lists/sentences are not split",
                              "fenced code remains an indivisible source block; not general Markdown parsing",
                              "no source bodies in report; no semantic quality or ingestion gate established"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"records": len(rows), "spans": sum(len(r["spans"]) for r in rows),
                      "segmentation_ms": report["segmentation_ms"], "inference_calls": 0}))


if __name__ == "__main__":
    main()
