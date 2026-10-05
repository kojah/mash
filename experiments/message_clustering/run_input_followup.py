"""Score the sixteen frozen follow-up cells on development judgments only."""

import hashlib
import io
import os
from enum import StrEnum
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from blind_turn_pairs import Verdict
from build_input_followup import Built
from corpus import Record
from followup_inputs import Variant
from run_study_development import (
    Interval,
    Prediction,
    Report,
    Row,
    bootstrap,
    summarize,
)
from study_annotations import load_development
from study_artifacts import View
from study_scoring import (
    Candidate,
    Decision,
    Family,
    JudgedPair,
    Summary,
    absolute_gate,
    combine,
    connections,
    fit,
    minimum_gain,
    score,
)
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase


class Mode(StrEnum):
    PRIMARY = "primary"
    CENTROID = "centroid"


class Cell(Record):
    variant: Variant
    cutoff: float
    radius: float
    mode: Mode

    @property
    def name(self) -> str:
        return f"{self.variant}:birch:{self.radius:.2f}:{self.cutoff:.2f}:{self.mode}"


CELLS = tuple(
    Cell(variant=variant, cutoff=cutoff, radius=radius, mode=mode)
    for variant in Variant
    for cutoff, radius in ((0.65, 0.75), (0.75, 0.60))
    for mode in Mode
)


class Gate(Record):
    name: str
    overall: Counts
    long_history: Counts
    quality_eligible: bool
    interval: Interval


class FollowupReport(Record):
    original_report_sha256: str
    registered_cells: tuple[Cell, ...]
    report: Report
    gates: tuple[Gate, ...]
    confirmation_status: str = "sealed; temporal experiment remains separate"


def choose(
    candidates: tuple[Summary, ...], rows: tuple[Row, ...], baseline: Summary
) -> Decision:
    eligible = tuple(
        candidate
        for candidate in candidates
        if absolute_gate(candidate.counts)
        and minimum_gain(candidate.counts, baseline.counts)
        and absolute_gate(
            combine(
                tuple(
                    row.counts
                    for row in rows
                    if row.name == candidate.name and row.prompted_turns >= 8
                )
            )
        )
    )
    selected = (
        min(eligible, key=lambda item: (-item.counts.f1, item.seconds, item.name))
        if eligible
        else None
    )
    return Decision(
        baseline=baseline.name,
        selected=selected.name if selected else None,
        requires_gain=True,
    )


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--original-report", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    artifacts: Path,
    original_report: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("follow-up result exists; do not overwrite")
    data = cohort.read_bytes()
    cohort_sha = hashlib.sha256(data).hexdigest()
    captured = Cohort.model_validate_json(data)
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    original_bytes = original_report.read_bytes()
    original = Report.model_validate_json(original_bytes)
    labels = load_development(annotations)
    if (
        original.cohort_sha256 != cohort_sha
        or labels.manifest.cohort_sha256 != cohort_sha
        or labels.manifest.protocol_sha256 != original.protocol_sha256
        or labels.file_sha256 != original.annotation_sha256
        or labels.manifest_sha256 != original.annotation_manifest_sha256
        or (artifacts / "cohort.sha256").read_text().strip() != cohort_sha
        or (artifacts / "protocol.sha256").read_text().strip() != protocol_sha
        or hashlib.sha256((artifacts / "encoder.json").read_bytes()).hexdigest()
        != original.encoder_manifest_sha256
    ):
        raise click.ClickException("follow-up provenance mismatch")
    if not (artifacts / "complete").is_file():
        raise click.ClickException("follow-up build is incomplete")
    verdicts = tuple(label.verdict for label in labels.labels.values())
    if (
        verdicts.count(Verdict.RELATED) < 100
        or verdicts.count(Verdict.UNRELATED) < 200
        or not labels.confirmation_adequate
    ):
        raise click.ClickException("registered annotation adequacy failed")
    if original.decision.baseline != "chronological_4":
        raise click.ClickException("frozen baseline must be chronological_4")
    rows = [row for row in original.rows if row.name == "chronological_4"]
    hashes = {}
    with threadpool_limits(limits=1):
        for entry in captured.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            thread = entry.captured
            owners = tuple(turn.id for turn in thread.turns if turn.prompts)
            refs = tuple(
                ref
                for ref in labels.manifest.references
                if ref.thread_index == thread.index and ref.phase == Phase.DEVELOPMENT
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
            for variant in Variant:
                name = f"{thread.index}-{variant}"
                provenance = (artifacts / f"{name}.json").read_bytes()
                built = Built.model_validate_json(provenance)
                raw = (artifacts / f"{name}.npz").read_bytes()
                matrix_sha = hashlib.sha256(raw).hexdigest()
                if (
                    built.thread_index != thread.index
                    or built.phase != entry.phase
                    or built.variant != variant
                    or built.turn_ids != owners
                    or built.matrix_sha256 != matrix_sha
                ):
                    raise click.ClickException("matrix/input provenance mismatch")
                if (
                    variant == Variant.RAW
                    and built.frozen_control_sha256
                    != original.artifact_sha256[f"{thread.index}-{View.SEPARATE}.npz"]
                ):
                    raise click.ClickException(
                        "raw control differs from original scored matrix"
                    )
                hashes[f"{name}.npz"] = matrix_sha
                hashes[f"{name}.json"] = hashlib.sha256(provenance).hexdigest()
                with np.load(io.BytesIO(raw), allow_pickle=False) as matrix:
                    vectors = matrix["vectors"]
                    if tuple(matrix["turn_ids"].tolist()) != owners:
                        raise click.ClickException("matrix turn identities differ")
                if (
                    vectors.shape != (len(owners), 384)
                    or not np.isfinite(vectors).all()
                ):
                    raise click.ClickException("invalid follow-up vectors")
                for cutoff, radius in ((0.65, 0.75), (0.75, 0.60)):
                    assigned = fit(
                        vectors,
                        owners,
                        Candidate(
                            view=View.SEPARATE,
                            family=Family.BIRCH,
                            cutoff=cutoff,
                            radius=radius,
                        ),
                    )
                    primary = connections(pairs, assigned.primary)
                    for mode in Mode:
                        memberships = (
                            assigned.primary
                            if mode == Mode.PRIMARY
                            else assigned.overlapping
                        )
                        predictions = connections(pairs, memberships)
                        cell = Cell(
                            variant=variant, cutoff=cutoff, radius=radius, mode=mode
                        )
                        rows.append(
                            Row(
                                name=cell.name,
                                thread_index=thread.index,
                                provider=thread.thread.provider.value,
                                node=entry.node,
                                prompted_turns=len(owners),
                                counts=score(pairs, predictions),
                                primary_counts=score(pairs, primary),
                                seconds=assigned.seconds,
                                group_sizes=assigned.group_sizes,
                                assigned_turns=sum(
                                    bool(groups) for groups in memberships.values()
                                ),
                                predictions=tuple(
                                    Prediction(
                                        pair_id=ref.pair_id,
                                        stratum=ref.stratum,
                                        batch=ref.batch,
                                        overlapping=predictions[ref.pair_id],
                                        primary=primary[ref.pair_id],
                                    )
                                    for ref in refs
                                ),
                            )
                        )
                        if variant == Variant.RAW:
                            original_name = Candidate(
                                view=View.SEPARATE,
                                family=Family.BIRCH,
                                cutoff=cutoff,
                                radius=radius,
                            ).name
                            previous = next(
                                row
                                for row in original.rows
                                if row.name == original_name
                                and row.thread_index == thread.index
                            )
                            expected = (
                                previous.primary_counts
                                if mode == Mode.PRIMARY
                                else previous.counts
                            )
                            if rows[-1].counts != expected:
                                raise click.ClickException(
                                    "raw control does not reproduce original counts"
                                )
            click.echo(f"Scored follow-up development thread {thread.index}")
    frozen_rows = tuple(rows)
    summaries = tuple(summarize(frozen_rows, cell.name) for cell in CELLS)
    baseline = summarize(frozen_rows, "chronological_4")
    decision = choose(summaries, frozen_rows, baseline)
    baseline_counts = tuple(
        row.counts for row in frozen_rows if row.name == baseline.name
    )
    gates = tuple(
        Gate(
            name=summary.name,
            overall=summary.counts,
            long_history=combine(
                tuple(
                    row.counts
                    for row in frozen_rows
                    if row.name == summary.name and row.prompted_turns >= 8
                )
            ),
            quality_eligible=absolute_gate(summary.counts)
            and minimum_gain(summary.counts, baseline.counts)
            and absolute_gate(
                combine(
                    tuple(
                        row.counts
                        for row in frozen_rows
                        if row.name == summary.name and row.prompted_turns >= 8
                    )
                )
            ),
            interval=bootstrap(
                tuple(row.counts for row in frozen_rows if row.name == summary.name),
                baseline_counts,
            ),
        )
        for summary in summaries
    )
    report = FollowupReport(
        original_report_sha256=hashlib.sha256(original_bytes).hexdigest(),
        registered_cells=CELLS,
        gates=gates,
        report=Report(
            cohort_sha256=cohort_sha,
            protocol_sha256=protocol_sha,
            annotation_sha256=labels.file_sha256,
            annotation_manifest_sha256=labels.manifest_sha256,
            artifact_sha256=hashes,
            encoder_manifest_sha256=original.encoder_manifest_sha256,
            agreement=labels.agreement,
            candidates=summaries,
            baselines=(baseline,),
            decision=decision,
            selected_interval=next(
                (gate.interval for gate in gates if gate.name == decision.selected),
                None,
            ),
            rows=frozen_rows,
        ),
    )
    with output.open("xb") as destination:
        destination.write(orjson.dumps(report.model_dump(mode="json")))
    click.echo(
        f"Follow-up quality choice: {decision.selected}; confirmation remains sealed"
    )


if __name__ == "__main__":
    main()
