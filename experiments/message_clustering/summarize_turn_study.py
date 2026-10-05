"""Export registered breakdowns after frozen scoring, without fitting new models."""

import hashlib
import os
from pathlib import Path

import click
import orjson

from confirm_turn_study import Confirmation, unseal
from corpus import Record
from run_study_development import Report, Row
from study_analysis import BuildCost, FitCost, Slice, build_costs, fit_costs, slices
from study_annotations import load_development
from study_artifacts import Cost
from turn_study_corpus import Phase


class Breakdown(Record):
    phase: Phase
    scores: tuple[Slice, ...]
    clustering: tuple[FitCost, ...]


class Analysis(Record):
    development_sha256: str
    confirmation_sha256: str | None
    costs_sha256: str
    breakdowns: tuple[Breakdown, ...]
    construction: tuple[BuildCost, ...]
    construction_description: str = "cache-assisted; not fully cold inference"


def verify_rows(
    rows: tuple[Row, ...], indices: frozenset[int], names: frozenset[str]
) -> None:
    expected = {(index, name) for index in indices for name in names}
    actual = {(row.thread_index, row.name) for row in rows}
    if expected != actual or len(rows) != len(expected):
        raise ValueError("score rows must cover each configuration/thread exactly once")


@click.command()
@click.option(
    "--development", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--confirmation", type=click.Path(path_type=Path, exists=True))
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--costs", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    development: Path,
    confirmation: Path | None,
    annotations: Path,
    costs: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    raw = development.read_bytes()
    report = Report.model_validate_json(raw)
    labels = load_development(annotations)
    if (
        labels.manifest_sha256 != report.annotation_manifest_sha256
        or labels.file_sha256 != report.annotation_sha256
    ):
        raise click.ClickException("annotations changed after the development choice")
    refs = {ref.pair_id: ref for ref in labels.manifest.references}
    indices = frozenset(
        ref.thread_index for ref in refs.values() if ref.phase == Phase.DEVELOPMENT
    )
    names = frozenset(
        summary.name for summary in (*report.candidates, *report.baselines)
    )
    verify_rows(report.rows, indices, names)
    breakdowns = [
        Breakdown(
            phase=Phase.DEVELOPMENT,
            scores=slices(report.rows, labels.labels, refs),
            clustering=fit_costs(report.rows),
        )
    ]
    confirmation_sha = None
    if confirmation:
        confirmation_raw = confirmation.read_bytes()
        selected = Confirmation.model_validate_json(confirmation_raw)
        if selected.development_sha256 != hashlib.sha256(raw).hexdigest():
            raise click.ClickException(
                "confirmation uses a different development choice"
            )
        manifest, confirmation_labels = unseal(annotations, report)
        confirmation_refs = {
            ref.pair_id: ref
            for ref in manifest.references
            if ref.phase == Phase.CONFIRMATION
        }
        verify_rows(
            selected.rows,
            frozenset(ref.thread_index for ref in confirmation_refs.values()),
            frozenset((selected.selected.name, selected.baseline.name)),
        )
        breakdowns.append(
            Breakdown(
                phase=Phase.CONFIRMATION,
                scores=slices(selected.rows, confirmation_labels, confirmation_refs),
                clustering=fit_costs(selected.rows),
            )
        )
        confirmation_sha = hashlib.sha256(confirmation_raw).hexdigest()
    cost_bytes = costs.read_bytes()
    observations = tuple(
        Cost.model_validate_json(line) for line in cost_bytes.splitlines() if line
    )
    analysis = Analysis(
        development_sha256=hashlib.sha256(raw).hexdigest(),
        confirmation_sha256=confirmation_sha,
        costs_sha256=hashlib.sha256(cost_bytes).hexdigest(),
        breakdowns=tuple(breakdowns),
        construction=build_costs(
            observations, frozenset(ref.thread_index for ref in refs.values())
        ),
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as destination:
        destination.write(orjson.dumps(analysis.model_dump(mode="json")))
    click.echo(f"Wrote {len(analysis.breakdowns)} phase breakdowns; no new fits")


if __name__ == "__main__":
    main()
