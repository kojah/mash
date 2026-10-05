"""Four frozen ensemble cells scored from parent predictions; no refitting."""

import hashlib
import os
import time
from pathlib import Path

import click
import orjson

from corpus import Record
from run_input_followup import FollowupReport, Gate, choose
from run_structure_followup import StructureReport
from run_study_development import Prediction, Report, Row, bootstrap, summarize
from study_analysis import FitCost, Slice, fit_costs, slices
from study_annotations import DevelopmentLabels, load_development
from study_scoring import JudgedPair, absolute_gate, combine, minimum_gain, score
from turn_ensemble import Mode, predictions
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase

A_SHA = "d95bb0cb317a40fcd107db4ac6ff4074f66c76a21b87301d22a802f1f2b6bf1e"
B_SHA = "50d7ef826e4d21196dc4e3cef5c33d59a5f2884561c26cb2e2ec723d3ca28555"
A_NAME = "context_whole:birch:0.60:0.75:centroid"
B_NAME = "structure_heading:birch:0.75"


class Diagnostic(Record):
    name: str
    thread_index: int
    nearby: Counts
    distant: Counts


class EnsembleReport(Record):
    parent_a_sha256: str
    parent_b_sha256: str
    report: Report
    gates: tuple[Gate, ...]
    scores: tuple[Slice, ...]
    conservative_parent_costs: tuple[FitCost, ...]
    diagnostics: tuple[Diagnostic, ...]
    membership_status: str = (
        "recorded pair operators only; eligible choice requires "
        "cached-membership recomputation"
    )
    confirmation_status: str = "sealed"
    timing_description: str = (
        "historical parent fit/preparation costs plus combination; "
        "not end-to-end recomputation timing"
    )


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--parent-a", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--parent-b", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    parent_a: Path,
    parent_b: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("ensemble result exists; do not overwrite")
    a_raw, b_raw = parent_a.read_bytes(), parent_b.read_bytes()
    if (
        hashlib.sha256(a_raw).hexdigest() != A_SHA
        or hashlib.sha256(b_raw).hexdigest() != B_SHA
    ):
        raise click.ClickException("frozen parent report changed")
    a_report = FollowupReport.model_validate_json(a_raw).report
    b_report = StructureReport.model_validate_json(b_raw).report
    data = cohort.read_bytes()
    cohort_sha = hashlib.sha256(data).hexdigest()
    captured = Cohort.model_validate_json(data)
    labels = load_development(annotations)
    result = score_ensemble(
        a_report,
        b_report,
        captured,
        labels,
        cohort_sha,
        hashlib.sha256(protocol.read_bytes()).hexdigest(),
        A_SHA,
        B_SHA,
        A_NAME,
        B_NAME,
        tuple(Mode),
    )
    with output.open("xb") as destination:
        destination.write(orjson.dumps(result.model_dump(mode="json")))
    click.echo(
        f"Ensemble quality choice: {result.report.decision.selected}; "
        "membership verification precedes confirmation"
    )


def score_ensemble(
    a_report: Report,
    b_report: Report,
    captured: Cohort,
    labels: DevelopmentLabels,
    cohort_sha: str,
    protocol_sha: str,
    a_sha: str,
    b_sha: str,
    a_name: str,
    b_name: str,
    modes: tuple[Mode, ...],
) -> EnsembleReport:
    """Shared scoring only; each caller pins its prospective parents and cells."""
    if (
        a_report.cohort_sha256 != cohort_sha
        or b_report.cohort_sha256 != cohort_sha
        or a_report.annotation_manifest_sha256 != labels.manifest_sha256
        or b_report.annotation_manifest_sha256 != labels.manifest_sha256
        or a_report.annotation_sha256 != labels.file_sha256
        or b_report.annotation_sha256 != labels.file_sha256
        or a_report.encoder_manifest_sha256 != b_report.encoder_manifest_sha256
    ):
        raise click.ClickException("parent cohort/model/annotation provenance differs")
    rows = [row for row in a_report.rows if row.name == "chronological_4"]
    diagnostics = []
    for entry in captured.entries:
        if entry.phase != Phase.DEVELOPMENT:
            continue
        index = entry.captured.index
        a = next(
            row
            for row in a_report.rows
            if row.thread_index == index and row.name == a_name
        )
        b = next(
            row
            for row in b_report.rows
            if row.thread_index == index and row.name == b_name
        )
        if (a.provider, a.node, a.prompted_turns) != (
            b.provider,
            b.node,
            b.prompted_turns,
        ):
            raise click.ClickException("parent row identities differ")
        positions = {
            turn.id: position
            for position, turn in enumerate(
                tuple(turn for turn in entry.captured.turns if turn.prompts)
            )
        }
        refs = tuple(
            ref
            for ref in labels.manifest.references
            if ref.thread_index == index and ref.phase == Phase.DEVELOPMENT
        )
        pairs = tuple(
            JudgedPair(
                ref.pair_id,
                ref.a_turn_id,
                ref.b_turn_id,
                labels.labels[ref.pair_id].verdict,
            )
            for ref in refs
        )
        near = tuple(
            pair for pair in pairs if abs(positions[pair.a] - positions[pair.b]) <= 4
        )
        far = tuple(
            pair for pair in pairs if abs(positions[pair.a] - positions[pair.b]) > 4
        )
        left = {
            prediction.pair_id: prediction.overlapping for prediction in a.predictions
        }
        right = {
            prediction.pair_id: prediction.overlapping for prediction in b.predictions
        }
        if score(pairs, left) != a.counts or score(pairs, right) != b.counts:
            raise click.ClickException("parent predictions do not reproduce counts")
        for mode in modes:
            started = time.perf_counter()
            combined = predictions(left, right, mode)
            primary = predictions(
                {p.pair_id: p.primary for p in a.predictions},
                {p.pair_id: p.primary for p in b.predictions},
                mode,
            )
            operation = time.perf_counter() - started
            cost = (
                a.seconds
                if mode == Mode.A
                else b.seconds
                if mode == Mode.B
                else a.seconds + b.seconds
            )
            rows.append(
                Row(
                    name=mode.value,
                    thread_index=index,
                    provider=a.provider,
                    node=a.node,
                    prompted_turns=a.prompted_turns,
                    counts=score(pairs, combined),
                    primary_counts=score(pairs, primary),
                    seconds=cost + operation,
                    group_sizes=a.group_sizes
                    if mode == Mode.A
                    else b.group_sizes
                    if mode == Mode.B
                    else (),
                    assigned_turns=a.assigned_turns
                    if mode == Mode.A
                    else b.assigned_turns
                    if mode == Mode.B
                    else None,
                    predictions=tuple(
                        Prediction(
                            pair_id=ref.pair_id,
                            stratum=ref.stratum,
                            batch=ref.batch,
                            overlapping=combined[ref.pair_id],
                            primary=primary[ref.pair_id],
                        )
                        for ref in refs
                    ),
                )
            )
            diagnostics.append(
                Diagnostic(
                    name=mode.value,
                    thread_index=index,
                    nearby=score(near, {pair.id: combined[pair.id] for pair in near}),
                    distant=score(far, {pair.id: combined[pair.id] for pair in far}),
                )
            )
    frozen_rows = tuple(rows)
    summaries = tuple(summarize(frozen_rows, mode.value) for mode in modes)
    baseline = summarize(frozen_rows, "chronological_4")
    decision = choose(summaries, frozen_rows, baseline)
    baseline_counts = tuple(
        row.counts for row in frozen_rows if row.name == baseline.name
    )
    gates = []
    for summary in summaries:
        long = combine(
            tuple(
                row.counts
                for row in frozen_rows
                if row.name == summary.name and row.prompted_turns >= 8
            )
        )
        gates.append(
            Gate(
                name=summary.name,
                overall=summary.counts,
                long_history=long,
                quality_eligible=absolute_gate(summary.counts)
                and absolute_gate(long)
                and minimum_gain(summary.counts, baseline.counts),
                interval=bootstrap(
                    tuple(
                        row.counts for row in frozen_rows if row.name == summary.name
                    ),
                    baseline_counts,
                ),
            )
        )
    result = EnsembleReport(
        parent_a_sha256=a_sha,
        parent_b_sha256=b_sha,
        gates=tuple(gates),
        scores=slices(
            frozen_rows,
            labels.labels,
            {ref.pair_id: ref for ref in labels.manifest.references},
        ),
        conservative_parent_costs=fit_costs(frozen_rows),
        diagnostics=tuple(diagnostics),
        report=Report(
            cohort_sha256=cohort_sha,
            protocol_sha256=protocol_sha,
            annotation_sha256=labels.file_sha256,
            annotation_manifest_sha256=labels.manifest_sha256,
            artifact_sha256={},
            encoder_manifest_sha256=a_report.encoder_manifest_sha256,
            agreement=labels.agreement,
            candidates=summaries,
            baselines=(baseline,),
            decision=decision,
            rows=frozen_rows,
            selected_interval=next(
                (gate.interval for gate in gates if gate.name == decision.selected),
                None,
            ),
        ),
    )
    return result


if __name__ == "__main__":
    main()
