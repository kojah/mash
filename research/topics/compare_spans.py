"""Compare complete matched-unit captures, excluding only measured runtime fields."""
import argparse
import json
from pathlib import Path

from reference_review import digest
from unit_grouping import Geometry, Method, Representation


MEASUREMENTS = frozenset({"encoding_ms", "feature_ms", "pooling_ms", "grouping_ms", "labeling_ms",
                          "evaluation_ms", "elapsed_ms_after_imports", "peak_rss_mib",
                          "inference_ms", "tokenization_ms"})


def stable(value):
    if isinstance(value, dict):
        return {k: stable(v) for k, v in value.items() if k not in MEASUREMENTS}
    if isinstance(value, list):
        return [stable(v) for v in value]
    return value


def compare(first: dict, second: dict) -> dict:
    expected = {(g.value, r.value, m.value, reverse) for g in Geometry for r in Representation
                for m in Method for reverse in (False, True)}
    for capture in (first, second):
        if (capture.get("schema_version") != 1
                or capture.get("scope") != "development-matched-source-unit-grouping"):
            raise ValueError("unsupported matched-unit capture")
        actual = [(p["geometry"], p["representation"], p["method"], p["reverse"]) for p in capture["reports"]]
        if len(actual) != len(expected) or set(actual) != expected:
            raise ValueError("incomplete or duplicated experiment grid")
    if stable(first) != stable(second):
        raise ValueError("nonmeasurement fields differ between captures")
    return {"schema_version": 1, "scope": "matched-source-unit-repeat-verification",
            "identical_nonmeasurement_fields": True, "controls": len(expected),
            "dataset_fingerprint": first["dataset_fingerprint"],
            "elapsed_ms": [r["elapsed_ms_after_imports"] for r in (first, second)],
            "peak_rss_mib": [r["peak_rss_mib"] for r in (first, second)]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("first", "second", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("capture summaries must remain outside Git")
    raw = [path.read_bytes() for path in (args.first, args.second)]
    summary = {**compare(*(json.loads(r) for r in raw)), "capture_sha256": [digest(r) for r in raw]}
    with args.output.open("x") as output:
        json.dump(summary, output, indent=2, allow_nan=False)
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
