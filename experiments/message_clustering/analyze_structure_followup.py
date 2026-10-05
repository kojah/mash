"""Registered construction/fragmentation evidence, without new fits or labels."""

import hashlib
import os
from collections import Counter
from pathlib import Path

import click
import numpy as np
import orjson

from analyze_input_followup import Resources, peak_rss
from build_structure_followup import StructuredArtifact
from corpus import Record
from run_structure_followup import StructureReport
from structure_chunks import Kind, Mode
from turn_study_corpus import Cohort


class Distribution(Record):
    mode: Mode
    histories: int
    turns: int
    points: int
    points_per_turn_p50: float
    points_per_turn_p95: float
    points_per_turn_max: int
    input_tokens_p50: float
    input_tokens_p95: float
    inputs_under_16_tokens: int
    forced_token_splits: int
    kinds: dict[Kind, int]
    new_vectors: int
    construction_p95_seconds: float


def distribution(mode: Mode, artifacts: tuple[StructuredArtifact, ...]) -> Distribution:
    if not artifacts or any(artifact.mode != mode for artifact in artifacts):
        raise ValueError("distribution needs one nonempty representation")
    counts = [
        count
        for artifact in artifacts
        for count in Counter(unit.input.turn_id for unit in artifact.units).values()
    ]
    lengths = [unit.input.tokens for artifact in artifacts for unit in artifact.units]
    return Distribution(
        mode=mode,
        histories=len(artifacts),
        turns=sum(len(artifact.turn_ids) for artifact in artifacts),
        points=len(lengths),
        points_per_turn_p50=float(np.quantile(counts, 0.50)),
        points_per_turn_p95=float(np.quantile(counts, 0.95)),
        points_per_turn_max=max(counts),
        input_tokens_p50=float(np.quantile(lengths, 0.50)),
        input_tokens_p95=float(np.quantile(lengths, 0.95)),
        inputs_under_16_tokens=sum(length < 16 for length in lengths),
        forced_token_splits=sum(
            unit.forced_token_split for artifact in artifacts for unit in artifact.units
        ),
        kinds=dict(
            Counter(unit.kind for artifact in artifacts for unit in artifact.units)
        ),
        new_vectors=sum(artifact.new_vectors for artifact in artifacts),
        construction_p95_seconds=float(
            np.quantile([artifact.seconds for artifact in artifacts], 0.95)
        ),
    )


class Analysis(Record):
    development_sha256: str
    distributions: tuple[Distribution, ...]
    resources: Resources
    construction_gate_passed: bool
    preparation_fit_gate_passed: bool
    scope: str = (
        "construction/distributions: all 200 histories; fit timing: development only"
    )
    token_description: str = (
        "input tokens include any ancestor and heading context, not body-only lengths"
    )
    construction_description: str = (
        "cache-assisted with sequential cache reuse; not independent cold builds"
    )


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--development", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--build-time", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--score-time", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    development: Path,
    artifacts: Path,
    build_time: Path,
    score_time: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    raw = development.read_bytes()
    report = StructureReport.model_validate_json(raw)
    data = cohort.read_bytes()
    if hashlib.sha256(data).hexdigest() != report.report.cohort_sha256:
        raise click.ClickException("construction cohort changed")
    captured = Cohort.model_validate_json(data)
    distributions = []
    for mode in Mode:
        observations = []
        for entry in captured.entries:
            name = f"{entry.captured.index}-{mode}.json"
            provenance = (artifacts / name).read_bytes()
            if (
                name in report.report.artifact_sha256
                and hashlib.sha256(provenance).hexdigest()
                != report.report.artifact_sha256[name]
            ):
                raise click.ClickException("scored structural provenance changed")
            built = StructuredArtifact.model_validate_json(provenance)
            if (
                built.thread_index != entry.captured.index
                or built.phase != entry.phase
                or built.mode != mode
                or built.turn_ids
                != tuple(turn.id for turn in entry.captured.turns if turn.prompts)
            ):
                raise click.ClickException("structural construction identities differ")
            observations.append(built)
        distributions.append(distribution(mode, tuple(observations)))
    build_peak, score_peak = peak_rss(build_time), peak_rss(score_time)
    result = Analysis(
        development_sha256=hashlib.sha256(raw).hexdigest(),
        distributions=tuple(distributions),
        resources=Resources(
            build_peak_kib=build_peak,
            score_peak_kib=score_peak,
            memory_gate_passed=max(build_peak, score_peak) <= 4 * 1024**2,
        ),
        construction_gate_passed=all(
            item.construction_p95_seconds <= 300 for item in distributions
        ),
        preparation_fit_gate_passed=all(
            item.p95_seconds <= 10 for item in report.clustering
        ),
    )
    with output.open("xb") as destination:
        destination.write(orjson.dumps(result.model_dump(mode="json")))
    click.echo("Wrote vector distributions and terminal resource gates; no new fits")


if __name__ == "__main__":
    main()
