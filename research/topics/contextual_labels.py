"""Oracle resolution-aware label inputs; no automatic classifier or inference."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
import json
from pathlib import Path
import re
import time

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer

from context_selection import bounded_exchange
from episodes import read_cases
from lexical_boundaries import admissions
from phrase_labels import source_terms, supported
from reference_review import Resolution, digest, validate
from subject_labels import label_scores, ranked_terms


class Mode(StrEnum):
    ALL = "all-admitted-user-support"
    CONTENT = "content-only-support"
    CONTEXT = "content-plus-resolved-task-exchange"
    GUARDED = "content-plus-task-exchange-negation-veto"


NEGATION_MARKERS = frozenset({"no", "not", "never", "instead", "rather"})
WORDS = re.compile(r"(?u)\b\w+(?:['’]\w+)?\b")


def negation_veto(text: str) -> bool:
    words = WORDS.findall(text.casefold())
    return any(word in NEGATION_MARKERS or word.endswith(("n't", "n’t")) for word in words)


@dataclass(frozen=True)
class Unit:
    ordinal: int
    role: str
    text: str


def units(ordinal: int, text: str, kind: str, resolution: Resolution | None,
          context: dict, mode: Mode, kinds: dict[int, str]) -> tuple[Unit, ...]:
    anchor = Unit(ordinal, "user", text)
    if kind == "content" or mode == Mode.ALL:
        return (anchor,)
    if mode == Mode.CONTENT or resolution != Resolution.TASK:
        return ()
    if mode == Mode.GUARDED and negation_veto(text):
        return (anchor,)
    entries = bounded_exchange(context)
    if not entries:
        return ()
    # Previous generated/user metadata frames do not become label documents.
    preceding = [Unit(e["ordinal"], e["role"], e["text"]) for e in entries
                 if e["role"] == "assistant" or kinds.get(e["ordinal"]) == "content"]
    return tuple(preceding) + (anchor,)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "contexts", "reference-review", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private reports must stay outside Git")
    started = time.perf_counter()
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    admitted = admissions(cases, args.source_review)
    context_bytes, review_bytes, reference_bytes = (p.read_bytes() for p in (args.contexts, args.source_review, args.reference_review))
    judgments = validate(context_bytes, review_bytes, json.loads(reference_bytes))
    context_report = json.loads(context_bytes)
    if context_report["dataset_fingerprint"] != fingerprint:
        raise ValueError("context dataset changed")
    contexts = {(c["thread"], x["anchor"]["ordinal"]): x for c in context_report["cases"] for x in c["contexts"]}
    resolutions = {(j.thread, j.ordinal): j.resolution for j in judgments}
    review = json.loads(review_bytes)
    by_source = {c["thread"]: c for c in review["cases"]}
    by_thread = {c.thread: {t.ordinal: t for t in c.turns} for c in cases}
    reports = []
    for mode in Mode:
        start = time.perf_counter()
        selected, all_units, vetoed = {}, {}, []
        for case in cases:
            kinds = {j["ordinal"]: j["kind"] for j in by_source[case.thread]["admission_judgments"]}
            for turn in case.turns:
                if turn.id not in admitted:
                    continue
                context = contexts[(case.thread, turn.ordinal)]
                if context["anchor"]["id"] != turn.id or context["anchor"]["searchable_sha256"] != digest(turn.text.encode()):
                    raise ValueError("context anchor changed")
                documents = units(turn.ordinal, turn.text, kinds[turn.ordinal], resolutions.get((case.thread, turn.ordinal)), context, mode, kinds)
                if (mode == Mode.GUARDED and kinds[turn.ordinal] == "referential"
                        and resolutions.get((case.thread, turn.ordinal)) == Resolution.TASK
                        and negation_veto(turn.text)):
                    required = next(j.evidence_ordinals for j in judgments if j.thread == case.thread and j.ordinal == turn.ordinal)
                    vetoed.append({"thread": case.thread, "ordinal": turn.ordinal,
                                   "required_evidence_records_not_borrowed": len(required)})
                selected[(case.thread, turn.ordinal)] = documents
                for unit in documents:
                    key = (case.thread, unit.ordinal)
                    if key in all_units and all_units[key] != unit:
                        raise ValueError("conflicting source unit")
                    all_units[key] = unit
        vectorizer = CountVectorizer(analyzer=source_terms, max_features=8192)
        keys = sorted(all_units)
        positions = {key: i for i, key in enumerate(keys)}
        background = vectorizer.fit_transform(all_units[key].text for key in keys).toarray()
        specs, support_keys = [], []
        for case in cases:
            for index, subject in enumerate(by_source[case.thread]["subjects"]):
                if subject["importance"] == "uncertain":
                    continue
                support = subject["support_ordinals"]
                if len(set(support)) != len(support) or any(o not in by_thread[case.thread] or (case.thread, o) not in selected for o in support):
                    raise ValueError("invalid source subject support")
                unique = sorted({(case.thread, u.ordinal) for o in support for u in selected[(case.thread, o)]})
                support_keys.append(unique)
                specs.append({"thread": case.thread, "subject_index": index, "support_ordinals": support,
                              "retained_support_anchors": sum(bool(selected[(case.thread, o)]) for o in support),
                              "units": [{"ordinal": all_units[k].ordinal, "role": all_units[k].role,
                                         "text_sha256": digest(all_units[k].text.encode())} for k in unique]})
        counts = np.stack([background[[positions[k] for k in group]].sum(axis=0) if group else np.zeros(background.shape[1]) for group in support_keys])
        scores, _ = label_scores(counts, background)
        terms = vectorizer.get_feature_names_out().tolist()
        for i, spec in enumerate(specs):
            spec["keywords"] = ranked_terms(scores[i], terms)
            if any(not supported(label["term"], [all_units[k].text for k in support_keys[i]]) for label in spec["keywords"]):
                raise ValueError("unsupported source phrase")
        reports.append({"mode": mode, "background_units": len(keys), "assistant_units": sum(u.role == "assistant" for u in all_units.values()),
                        "vocabulary": len(terms), "eligible_anchors": len(selected),
                        "retained_anchors": sum(bool(v) for v in selected.values()), "subjects": specs,
                        "vetoed_references": vetoed,
                        "fit_label_ms": (time.perf_counter() - start) * 1000})
    report = {"schema_version": 1, "scope": "development-oracle-resolution-label-inputs", "dataset_fingerprint": fingerprint,
              "input_sha256": {name: digest(getattr(args, name).read_bytes()) for name in ("annotations", "source_review", "contexts", "reference_review")},
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                                 ("contextual_labels.py", "context_selection.py", "phrase_labels.py", "subject_labels.py", "reference_review.py", "lexical_boundaries.py", "episodes.py", "data.py", "uv.lock")},
              "reports": reports, "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000, "inference_calls": 0,
              "negation_veto_policy": {"markers": sorted(NEGATION_MARKERS), "contractions": ["n't", "n’t"],
                                       "action": "retain user anchor, borrow no preceding text",
                                       "scope": "oracle original referential resolved-task anchors"},
              "limitations": ["oracle admission, subject supports and resolution labels; not automatic classification",
                              "preceding assistant proposals can include rejected or unendorsed entities",
                              "negation veto is an exposed fixed lexical control, not correction understanding",
                              "no reviewer-written names or rationales enter label inputs",
                              "source-preserving c-TF-IDF only; no semantic topic accuracy or tree evaluation",
                              "transductive vocabulary and class statistics; not online ingestion cost"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps([{k: v for k, v in r.items() if k != "subjects"} for r in reports]))


if __name__ == "__main__":
    main()
