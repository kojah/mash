"""Strict repeated bounded-fragment capture verification."""
import argparse
import json
from pathlib import Path

from compare_spans import stable
from fragments import Mode
from reference_review import digest
from unit_grouping import Method


def compare(first, second):
    for capture in (first, second):
        if (capture.get("schema_version") != 1
                or capture.get("scope") != "development-bounded-source-fragment-aspects"):
            raise ValueError("unsupported fragment capture")
        modes = [c["mode"] for c in capture["captures"]]
        if len(modes) != len(Mode) or set(modes) != set(Mode):
            raise ValueError("incomplete or duplicate fragment modes")
        for mode in capture["captures"]:
            keys = [(r["method"], r["reverse"]) for r in mode["reports"]]
            expected = {(m, reverse) for m in Method for reverse in (False, True)}
            if len(keys) != len(expected) or set(keys) != expected:
                raise ValueError("incomplete or duplicate fragment controls")
    if stable(first) != stable(second):
        raise ValueError("fragment capture nonmeasurement fields differ")
    return {"schema_version": 1, "scope": "bounded-fragment-repeat-verification",
            "identical_nonmeasurement_fields": True, "controls": len(Mode) * len(Method) * 2,
            "elapsed_ms": [c["elapsed_ms_after_imports"] for c in (first, second)],
            "cpu_ms": [c["cpu_ms"] for c in (first, second)],
            "peak_rss_mib": [c["peak_rss_mib"] for c in (first, second)]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("first", "second", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("use private artifact output")
    raw = [p.read_bytes() for p in (args.first, args.second)]
    summary = {**compare(*(json.loads(r) for r in raw)), "capture_sha256": [digest(r) for r in raw]}
    with args.output.open("x") as output:
        json.dump(summary, output, indent=2, allow_nan=False)
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
