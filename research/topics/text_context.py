"""Re-embedded single-turn control versus disjoint two-turn text contexts."""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import resource
import platform
import time

from episodes import read_cases, score
from local_embeddings import Encoder


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "snapshot", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError("use a fresh output path")
    started = time.perf_counter()
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    encoder = Encoder(args.snapshot)
    reports, drift = [], []
    for method in ("reembedded_adjacent055", "reembedded_disjoint_text2_055"):
        judgments, choices, details = [], [], []
        for case in cases:
            boundaries = []
            if method == "reembedded_adjacent055":
                for turn in case.turns:
                    if turn.vector is not None:
                        drift.append({"thread": case.thread, "message": turn.id,
                                      "stored_vs_reembedded_cosine": float(turn.vector @ encoder.encode(turn.text))})
            for i, annotation in enumerate(case.boundaries, start=1):
                if method == "reembedded_adjacent055":
                    left, right = case.turns[i - 1:i], case.turns[i:i + 1]
                else:
                    left, right = case.turns[max(0, i - 2):i], case.turns[i:i + 2]
                left_text, right_text = ("\n\n".join(t.text for t in block) for block in (left, right))
                cosine = float(encoder.encode(left_text) @ encoder.encode(right_text))
                choice = cosine < 0.55
                judgments.append(annotation.judgment)
                choices.append(choice)
                boundaries.append(dict(asdict(annotation), predicted=choice, cosine=cosine,
                                       left_messages=[t.id for t in left], right_messages=[t.id for t in right]))
            details.append({"thread": case.thread, "provider": case.provider,
                            "source_sha256": case.source_sha256, "boundaries": boundaries})
        reports.append({"method": method, "metrics": score(judgments, choices), "cases": details})
    peak_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if platform.system() != "Darwin":
        peak_rss *= 1024
    sources = ("text_context.py", "local_embeddings.py", "episodes.py", "data.py", "uv.lock")
    code_hashes = {name: hashlib.sha256((Path(__file__).parent / name).read_bytes()).hexdigest() for name in sources}
    report = {"schema_version": 1, "scope": "development_linear_user_text_context_pilot",
              "input_fingerprint": fingerprint, "annotations_sha256": hashlib.sha256(args.annotations.read_bytes()).hexdigest(),
              "policy": {"cosine_threshold": 0.55, "separator": "double_newline", "maximum_lookahead_users": 1,
                         "pooling": "singleton_attended_token_mean", "quantization": "symmetric_i8_per_vector",
                         "truncation": "reject", "assistant_text": False},
              "source_sha256": code_hashes, "encoder": asdict(encoder.statistics), "stored_vector_drift": drift, "reports": reports,
              "elapsed_ms": (time.perf_counter() - started) * 1000,
              "process_peak_rss_bytes": peak_rss}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
        output.write("\n")


if __name__ == "__main__":
    main()
