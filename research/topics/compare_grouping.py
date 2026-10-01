"""Validate paired grouping captures and report descriptive order diagnostics."""
import argparse
import json
from pathlib import Path

from sklearn.metrics import adjusted_rand_score

from grinch_controls import Method, Order, Source
from reference_review import digest


MEASUREMENTS = {"fit_wall_ms", "fit_cpu_ms", "insertion_ms", "evaluation_ms",
                "elapsed_ms_after_imports", "peak_rss_mib"}


def nonmeasurements(report: dict) -> dict:
    return {key: value for key, value in report.items() if key not in MEASUREMENTS}


def partition(report: dict) -> tuple[int, ...]:
    identities = report["canonical_identities"]
    labels = {}
    if identities != sorted(set(identities)):
        raise ValueError("duplicate or noncanonical identities")
    for i, cluster in enumerate(report["clusters"]):
        if not cluster:
            raise ValueError("empty cluster")
        for identity in cluster:
            if identity in labels:
                raise ValueError("duplicate partition membership")
            labels[identity] = i
    if set(labels) != set(identities):
        raise ValueError("partition does not cover exactly the same sources")
    return tuple(labels[i] for i in identities)


def compare(a: dict, b: dict) -> dict:
    for key in ("schema_version", "scope", "source", "method", "canonical_identities", "dimensions",
                "generation", "dataset_fingerprint", "partition_sha256", "parameters", "code_sha256",
                "versions", "reference_manifest_sha256"):
        if a[key] != b[key]:
            raise ValueError(f"incompatible comparison: {key}")
    result = {"native_cut_ari": float(adjusted_rand_score(partition(a), partition(b))),
              "triplet_agreement": None, "nontrivial_clade_jaccard": None}
    if a["hierarchy"] is not None:
        if b["hierarchy"] is None:
            raise ValueError("missing comparison hierarchy")
        x, y = a["hierarchy"]["triplet_signature"], b["hierarchy"]["triplet_signature"]
        if x["seed"] != y["seed"] or x["triplets"] != y["triplets"]:
            raise ValueError("triplet sampling changed")
        if len(x["closest_pairs"]) != len(x["triplets"]) or len(y["closest_pairs"]) != len(y["triplets"]):
            raise ValueError("incomplete triplet signature")
        if x["triplets"]:
            result["triplet_agreement"] = sum(p == q for p, q in zip(x["closest_pairs"], y["closest_pairs"])) / len(x["triplets"])
        x, y = set(a["hierarchy"]["clade_sha256"]), set(b["hierarchy"]["clade_sha256"])
        result["nontrivial_clade_jaccard"] = len(x & y) / len(x | y) if x or y else 1.
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--captures", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private comparisons must remain outside Git")
    hashes, summaries = {}, []
    for source in Source:
        for method in Method:
            first, all_reports = {}, []
            for order in Order:
                pair = []
                for repeat in range(2):
                    name = f"{source}-{method}-{order}-{repeat}.json"
                    raw = (args.captures / name).read_bytes()
                    hashes[name] = digest(raw)
                    report = json.loads(raw)
                    if (report["source"], report["method"], report["order"]) != (source, method, order):
                        raise ValueError("capture identity changed")
                    partition(report)
                    pair.append(report)
                    all_reports.append(report)
                if nonmeasurements(pair[0]) != nonmeasurements(pair[1]):
                    raise ValueError("fresh repeats differ outside measurements")
                first[order] = pair[0]
            baseline = first[Order.FORWARD]
            summaries.append({"source": source, "method": method,
                              "fit_ms_range": [min(r["fit_wall_ms"] for r in all_reports), max(r["fit_wall_ms"] for r in all_reports)],
                              "p95_insertion_ms_range": [min(r["insertion_ms"]["p95"] for r in all_reports), max(r["insertion_ms"]["p95"] for r in all_reports)],
                              "peak_rss_mib_range": [min(r["peak_rss_mib"] for r in all_reports), max(r["peak_rss_mib"] for r in all_reports)],
                              "clusters_by_order": {o: len(r["clusters"]) for o, r in first.items()},
                              "synthetic_purity_by_order": {o: r["hierarchy"]["dendrogram_purity"] if r["hierarchy"] else None for o, r in first.items()},
                              "order_comparisons": {o: compare(baseline, first[o]) for o in (Order.REVERSE, Order.SHUFFLED)}})
    result = {"schema_version": 1, "captures": len(hashes), "input_sha256": hashes,
              "comparator_sha256": digest(Path(__file__).read_bytes()), "summaries": summaries,
              "limitations": ["two fresh runs per method/order, descriptive timing ranges not confidence intervals",
                              "native-cut agreement across orders is not semantic correctness or cross-method equivalence",
                              "synthetic class purity is not real topic/ancestor quality or an ingestion gate"]}
    with args.output.open("x") as output:
        json.dump(result, output, indent=2, allow_nan=False)
    print(json.dumps({"captures": len(hashes), "matched_pairs": len(hashes) // 2, "sha256": digest(args.output.read_bytes())}))


if __name__ == "__main__":
    main()
