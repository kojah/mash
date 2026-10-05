"""Unseal confirmation only for the previously frozen development choice."""

import hashlib
import io
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from blind_turn_pairs import Annotations, Label, Manifest, Packet, validate
from corpus import Record
from run_study_development import (
    Interval,
    Prediction,
    Report,
    Row,
    bootstrap,
    summarize,
)
from study_scoring import (
    CANDIDATES,
    JudgedPair,
    Summary,
    absolute_gate,
    connections,
    fit,
    lexical_baselines,
    minimum_gain,
    score,
)
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase


class Confirmation(Record):
    development_sha256: str
    selected: Summary
    baseline: Summary
    quality_passes: bool
    accepted_by_quality: str | None
    requires_gain: bool
    interval: Interval
    artifact_sha256: dict[str, str]
    rows: tuple[Row, ...]


def accepted_choice(
    selected: Summary,
    baseline: Summary,
    requires_gain: bool,
    development_baseline: Counts,
) -> str | None:
    if absolute_gate(selected.counts) and (
        not requires_gain or minimum_gain(selected.counts, baseline.counts)
    ):
        return selected.name
    # Only the already frozen simple control may be a fallback, with both gates.
    if absolute_gate(development_baseline) and absolute_gate(baseline.counts):
        return baseline.name
    return None


def unseal(root: Path, report: Report) -> tuple[Manifest, dict[str, Label]]:
    """Reject missing selection and changed labels before decoding confirmation."""
    if report.decision.selected is None:
        raise ValueError("no development selection; confirmation stays closed")
    raw_manifest = (root / "manifest.json").read_bytes()
    if hashlib.sha256(raw_manifest).hexdigest() != report.annotation_manifest_sha256:
        raise ValueError("annotation manifest changed after development choice")
    manifest = Manifest.model_validate_json(raw_manifest)
    paths = {
        root / ("labels-" + path.name)
        for path in (*manifest.packets, *manifest.review_packets)
    }
    if {path.name for path in paths} != set(report.annotation_sha256):
        raise ValueError("annotation files changed after development choice")
    raw_labels = {}
    for path in paths:
        raw = path.read_bytes()
        if hashlib.sha256(raw).hexdigest() != report.annotation_sha256[path.name]:
            raise ValueError("annotation bytes changed after development choice")
        raw_labels[path.name] = raw
    references = {ref.pair_id: ref for ref in manifest.references}
    labels: dict[str, Label] = {}
    for path in manifest.packets:
        annotations = Annotations.model_validate_json(raw_labels["labels-" + path.name])
        validate(Packet.model_validate_json(path.read_bytes()), annotations)
        for label in annotations.labels:
            if references[label.pair_id].phase == Phase.CONFIRMATION:
                if label.pair_id in labels:
                    raise ValueError("duplicate confirmation pair")
                labels[label.pair_id] = label
    return manifest, labels


@click.command()
@click.option(
    "--development", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    development: Path, cohort: Path, annotations: Path, artifacts: Path, output: Path
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("confirmation output exists; no repeated evaluation")
    development_bytes = development.read_bytes()
    report = Report.model_validate_json(development_bytes)
    raw = cohort.read_bytes()
    if (
        hashlib.sha256(raw).hexdigest() != report.cohort_sha256
        or (artifacts / "cohort.sha256").read_text() != report.cohort_sha256
        or (artifacts / "protocol.sha256").read_text() != report.protocol_sha256
        or hashlib.sha256((artifacts / "encoder.json").read_bytes()).hexdigest()
        != report.encoder_manifest_sha256
    ):
        raise click.ClickException("confirmation provenance mismatch")
    # Pin the development inputs too: confirmation must not cover up changed DEV data.
    for name, checksum in report.artifact_sha256.items():
        if hashlib.sha256((artifacts / name).read_bytes()).hexdigest() != checksum:
            raise click.ClickException("development matrix changed after choice")
    manifest, labels = unseal(annotations, report)
    selected_name = report.decision.selected
    if selected_name is None:
        raise click.ClickException("development did not select a method")
    data = Cohort.model_validate_json(raw)
    rows: list[Row] = []
    matrix_sha256: dict[str, str] = {}
    names = tuple(dict.fromkeys((selected_name, report.decision.baseline)))
    with threadpool_limits(limits=1):
        for entry in data.entries:
            if entry.phase != Phase.CONFIRMATION:
                continue
            captured = entry.captured
            turns = tuple(turn for turn in captured.turns if turn.prompts)
            owners = tuple(turn.id for turn in turns)
            refs = tuple(
                ref
                for ref in manifest.references
                if ref.thread_index == captured.index and ref.pair_id in labels
            )
            pairs = tuple(
                JudgedPair(
                    ref.pair_id,
                    ref.a_turn_id,
                    ref.b_turn_id,
                    labels[ref.pair_id].verdict,
                )
                for ref in refs
            )
            for name in names:
                candidate = next((c for c in CANDIDATES if c.name == name), None)
                started = time.perf_counter()
                groups: tuple[int, ...] = ()
                assigned_turns = None
                if candidate:
                    filename = f"{captured.index}-{candidate.view.value}.npz"
                    matrix = (artifacts / filename).read_bytes()
                    matrix_sha256[filename] = hashlib.sha256(matrix).hexdigest()
                    with np.load(io.BytesIO(matrix), allow_pickle=False) as points:
                        vectors = points["vectors"]
                        point_owners = tuple(str(value) for value in points["turn_ids"])
                    if set(point_owners) != set(owners):
                        raise click.ClickException(
                            "confirmation turn identities mismatch"
                        )
                    assigned = fit(vectors, point_owners, candidate)
                    overlapping = connections(pairs, assigned.overlapping)
                    primary = connections(pairs, assigned.primary)
                    seconds = assigned.seconds
                    groups = assigned.group_sizes
                    assigned_turns = sum(
                        bool(group) for group in assigned.overlapping.values()
                    )
                else:
                    prompts = tuple(
                        "\n".join(
                            captured.thread.messages[i].projection for i in turn.prompts
                        )
                        for turn in turns
                    )
                    overlapping = lexical_baselines(prompts, owners, pairs)[name]
                    primary = overlapping
                    seconds = time.perf_counter() - started
                rows.append(
                    Row(
                        name=name,
                        thread_index=captured.index,
                        provider=captured.thread.provider.value,
                        node=entry.node,
                        prompted_turns=len(turns),
                        counts=score(pairs, overlapping),
                        primary_counts=score(pairs, primary),
                        seconds=seconds,
                        group_sizes=groups,
                        assigned_turns=assigned_turns,
                        predictions=tuple(
                            Prediction(
                                pair_id=ref.pair_id,
                                stratum=ref.stratum,
                                batch=ref.batch,
                                overlapping=overlapping[ref.pair_id],
                                primary=primary[ref.pair_id],
                            )
                            for ref in refs
                        ),
                    )
                )
    frozen_rows = tuple(rows)
    selected = summarize(frozen_rows, selected_name)
    baseline = summarize(frozen_rows, report.decision.baseline)
    confirmation = Confirmation(
        development_sha256=hashlib.sha256(development_bytes).hexdigest(),
        selected=selected,
        baseline=baseline,
        quality_passes=(
            absolute_gate(selected.counts)
            and (
                not report.decision.requires_gain
                or minimum_gain(selected.counts, baseline.counts)
            )
        ),
        accepted_by_quality=accepted_choice(
            selected,
            baseline,
            report.decision.requires_gain,
            next(
                value.counts
                for value in report.baselines
                if value.name == report.decision.baseline
            ),
        ),
        requires_gain=report.decision.requires_gain,
        interval=bootstrap(
            tuple(row.counts for row in rows if row.name == selected_name),
            tuple(row.counts for row in rows if row.name == report.decision.baseline),
        ),
        artifact_sha256=matrix_sha256,
        rows=frozen_rows,
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as destination:
        destination.write(orjson.dumps(confirmation.model_dump(mode="json")))
    click.echo(
        f"Single frozen-choice confirmation: "
        f"accepted_by_quality={confirmation.accepted_by_quality}"
    )


if __name__ == "__main__":
    main()
