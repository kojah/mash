"""Exactly one user-selected smoothed/chunk union and its fixed parent controls."""

import hashlib
import os
from pathlib import Path

import click
import orjson

from run_structure_followup import StructureReport
from run_temporal_smoothing import TemporalReport
from run_turn_ensemble import B_NAME, B_SHA, score_ensemble
from study_annotations import load_development
from turn_ensemble import Mode
from turn_study_corpus import Cohort

TEMPORAL_SHA = "d4ab356b33f2fbcd4905fc61fcceb2cb1d5793d1735eb1d088fe69ca0fedcb48"


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--temporal", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--structure", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    temporal: Path,
    structure: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("composite result exists; do not overwrite")
    left, right = temporal.read_bytes(), structure.read_bytes()
    if (
        hashlib.sha256(left).hexdigest() != TEMPORAL_SHA
        or hashlib.sha256(right).hexdigest() != B_SHA
    ):
        raise click.ClickException("fixed composite parents changed")
    raw = cohort.read_bytes()
    result = score_ensemble(
        TemporalReport.model_validate_json(left).report,
        StructureReport.model_validate_json(right).report,
        Cohort.model_validate_json(raw),
        load_development(annotations),
        hashlib.sha256(raw).hexdigest(),
        hashlib.sha256(protocol.read_bytes()).hexdigest(),
        TEMPORAL_SHA,
        B_SHA,
        "temporal:0.10:w4",
        B_NAME,
        (Mode.A, Mode.B, Mode.UNION),
    )
    with output.open("xb") as stream:
        stream.write(orjson.dumps(result.model_dump(mode="json")))
    click.echo(
        f"Smoothed/chunk quality choice: {result.report.decision.selected}; "
        "confirmation sealed"
    )


if __name__ == "__main__":
    main()
