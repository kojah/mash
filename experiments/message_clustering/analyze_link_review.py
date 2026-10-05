"""Validate blind judgments and summarize a purposive diagnostic separately."""

import hashlib
import os
from collections import Counter
from pathlib import Path

import click
import orjson

from blind_turn_pairs import Annotations, Label, Packet, Verdict, validate
from corpus import Record
from freeze_link_review import ReviewManifest


class Stratum(Record):
    reason: str
    long: bool
    near: bool
    related: int
    unrelated: int
    ambiguous: int


class Method(Record):
    name: str
    true_positive: int
    false_positive: int
    false_negative: int
    true_negative: int
    ambiguous: int


class Agreement(Record):
    pairs: int
    agreed: int
    disagreements: tuple[str, ...]


class ReviewResult(Record):
    manifest_sha256: str
    annotation_sha256: dict[str, str]
    pairs: int
    changed: int
    controls: int
    strata: tuple[Stratum, ...]
    methods: tuple[Method, ...]
    reviewer_sensitivity: tuple[Method, ...]
    agreement: Agreement
    limitation: str = (
        "Purposive changed-link sample with matched controls; counts are diagnostic, "
        "not population precision/recall, significance or confirmation results. "
        "Reviewer sensitivity substitutes only the independently reviewed subset; "
        "primary labels remain unchanged."
    )


def load_labels(paths: tuple[Path, ...]) -> tuple[dict[str, Label], dict[str, str]]:
    labels: dict[str, Label] = {}
    hashes = {}
    for path in paths:
        raw = path.with_name("labels-" + path.name).read_bytes()
        annotation = Annotations.model_validate_json(raw)
        packet = Packet.model_validate_json(path.read_bytes())
        validate(packet, annotation)
        for label in annotation.labels:
            if label.pair_id in labels:
                raise ValueError("duplicate annotation pair across packets")
            labels[label.pair_id] = label
        hashes[str(path.with_name("labels-" + path.name))] = hashlib.sha256(
            raw
        ).hexdigest()
    return labels, hashes


def methods(manifest: ReviewManifest, labels: dict[str, Label]) -> tuple[Method, ...]:
    counts: dict[str, Counter[str]] = {}
    for ref in manifest.references:
        verdict = labels[ref.draw.pair_id].verdict
        for prediction in ref.predictions:
            tally = counts.setdefault(prediction.cell.name, Counter())
            if verdict == Verdict.AMBIGUOUS:
                tally["ambiguous"] += 1
            elif verdict == Verdict.RELATED:
                tally["true_positive" if prediction.matched else "false_negative"] += 1
            else:
                tally["false_positive" if prediction.matched else "true_negative"] += 1
    return tuple(
        Method(
            name=name,
            true_positive=c["true_positive"],
            false_positive=c["false_positive"],
            false_negative=c["false_negative"],
            true_negative=c["true_negative"],
            ambiguous=c["ambiguous"],
        )
        for name, c in sorted(counts.items())
    )


def analyze(root: Path) -> ReviewResult:
    raw = (root / "manifest.json").read_bytes()
    manifest = ReviewManifest.model_validate_json(raw)
    for relative, expected in manifest.file_sha256.items():
        if hashlib.sha256((root / relative).read_bytes()).hexdigest() != expected:
            raise ValueError("frozen blind input hash changed")
    primary, hashes = load_labels(manifest.primary_packets)
    review, review_hashes = load_labels(manifest.review_packets)
    if set(primary) != {r.draw.pair_id for r in manifest.references}:
        raise ValueError("primary labels do not cover frozen references")
    if not set(review) <= set(primary):
        raise ValueError("review contains unknown pairs")
    strata: dict[tuple[str, bool, bool], Counter[Verdict]] = {}
    for ref in manifest.references:
        for reason in ref.draw.reasons:
            key = (reason.value, ref.long, ref.distance <= 4)
            strata.setdefault(key, Counter())[primary[ref.draw.pair_id].verdict] += 1
    disagreements = tuple(
        sorted(k for k, v in review.items() if v.verdict != primary[k].verdict)
    )
    controls = sum(bool(r.draw.control_for) for r in manifest.references)
    return ReviewResult(
        manifest_sha256=hashlib.sha256(raw).hexdigest(),
        annotation_sha256=hashes | review_hashes,
        pairs=len(primary),
        changed=len(primary) - controls,
        controls=controls,
        strata=tuple(
            Stratum(
                reason=reason,
                long=long,
                near=near,
                related=c[Verdict.RELATED],
                unrelated=c[Verdict.UNRELATED],
                ambiguous=c[Verdict.AMBIGUOUS],
            )
            for (reason, long, near), c in sorted(strata.items())
        ),
        methods=methods(manifest, primary),
        reviewer_sensitivity=methods(manifest, primary | review),
        agreement=Agreement(
            pairs=len(review),
            agreed=len(review) - len(disagreements),
            disagreements=disagreements,
        ),
    )


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(root: Path, output: Path) -> None:
    os.umask(0o077)
    result = analyze(root)
    output.write_bytes(orjson.dumps(result.model_dump(mode="json")))
    click.echo(
        f"Validated {result.pairs} primary and {result.agreement.pairs} review pairs"
    )


if __name__ == "__main__":
    main()
