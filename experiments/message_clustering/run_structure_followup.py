"""Score the four registered structural cells and two frozen window controls."""

import hashlib
import io
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from build_structure_followup import StructuredArtifact
from corpus import Record
from run_input_followup import Gate, choose
from run_study_development import Prediction, Report, Row, bootstrap, summarize
from run_window_followup import WindowReport
from structure_chunks import Mode
from study_analysis import FitCost, Slice, fit_costs, slices
from study_annotations import load_development
from study_artifacts import View
from study_scoring import (
    Candidate,
    Family,
    JudgedPair,
    absolute_gate,
    combine,
    connections,
    fit,
    minimum_gain,
    score,
)
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase

SOURCE_SHA = "126544f75c82034e0c400863311c918d00096a95878357721993d3322f404eba"


class Cell(Record):
    mode: Mode
    cutoff: float

    @property
    def name(self) -> str:
        return f"{self.mode}:birch:{self.cutoff:.2f}"


CELLS = tuple(
    Cell(mode=mode, cutoff=cutoff) for mode in Mode for cutoff in (0.65, 0.75)
)
CONTROLS = ("context_windows:birch:0.65", "context_windows:birch:0.75")


class Diagnostic(Record):
    name: str
    thread_index: int
    nearby: Counts
    distant: Counts
    points: int
    multi_membership_turns: int


class StructureReport(Record):
    source_development_sha256: str
    registered_cells: tuple[Cell, ...]
    report: Report
    gates: tuple[Gate, ...]
    scores: tuple[Slice, ...]
    clustering: tuple[FitCost, ...]
    diagnostics: tuple[Diagnostic, ...]
    confirmation_status: str = "sealed"


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
    "--source-development", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    artifacts: Path,
    source_development: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("structural result exists; do not overwrite")
    source_bytes = source_development.read_bytes()
    if hashlib.sha256(source_bytes).hexdigest() != SOURCE_SHA:
        raise click.ClickException("frozen window report changed")
    source = WindowReport.model_validate_json(source_bytes).report
    data = cohort.read_bytes()
    cohort_sha = hashlib.sha256(data).hexdigest()
    captured = Cohort.model_validate_json(data)
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    labels = load_development(annotations)
    if (
        cohort_sha != source.cohort_sha256
        or labels.manifest_sha256 != source.annotation_manifest_sha256
        or labels.file_sha256 != source.annotation_sha256
        or (artifacts / "cohort.sha256").read_text().strip() != cohort_sha
        or (artifacts / "protocol.sha256").read_text().strip() != protocol_sha
        or hashlib.sha256((artifacts / "encoder.json").read_bytes()).hexdigest()
        != source.encoder_manifest_sha256
    ):
        raise click.ClickException("structural provenance mismatch")
    if not (artifacts / "complete").is_file():
        raise click.ClickException("structural build incomplete")
    rows = [row for row in source.rows if row.name in (*CONTROLS, "chronological_4")]
    diagnostics = []
    hashes = {}
    with threadpool_limits(limits=1):
        for entry in captured.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            thread = entry.captured
            owners = tuple(turn.id for turn in thread.turns if turn.prompts)
            positions = {owner: index for index, owner in enumerate(owners)}
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
            near = tuple(
                pair
                for pair in pairs
                if abs(positions[pair.a] - positions[pair.b]) <= 4
            )
            far = tuple(
                pair for pair in pairs if abs(positions[pair.a] - positions[pair.b]) > 4
            )
            for mode in Mode:
                started = time.perf_counter()
                stem = f"{thread.index}-{mode}"
                provenance = (artifacts / f"{stem}.json").read_bytes()
                built = StructuredArtifact.model_validate_json(provenance)
                matrix_bytes = (artifacts / f"{stem}.npz").read_bytes()
                digest = hashlib.sha256(matrix_bytes).hexdigest()
                if (
                    built.matrix_sha256 != digest
                    or built.thread_index != thread.index
                    or built.phase != entry.phase
                    or built.mode != mode
                    or built.turn_ids != owners
                    or built.source_input_sha256
                    != source.artifact_sha256[f"{thread.index}-context_whole.json"]
                ):
                    raise click.ClickException(
                        "structural matrix/input provenance mismatch"
                    )
                hashes[f"{stem}.npz"] = digest
                hashes[f"{stem}.json"] = hashlib.sha256(provenance).hexdigest()
                with np.load(io.BytesIO(matrix_bytes), allow_pickle=False) as matrix:
                    vectors = matrix["vectors"]
                    point_owners = tuple(matrix["turn_ids"].tolist())
                if (
                    point_owners != tuple(unit.input.turn_id for unit in built.units)
                    or set(point_owners) != set(owners)
                    or vectors.shape != (len(point_owners), 384)
                    or not np.isfinite(vectors).all()
                ):
                    raise click.ClickException("invalid structural points")
                preparation = time.perf_counter() - started
                for cutoff in (0.65, 0.75):
                    cell = Cell(mode=mode, cutoff=cutoff)
                    assigned = fit(
                        vectors,
                        point_owners,
                        Candidate(
                            view=View.SEPARATE,
                            family=Family.BIRCH,
                            cutoff=cutoff,
                            radius=0.75 if cutoff == 0.65 else 0.60,
                        ),
                    )
                    predictions = connections(pairs, assigned.overlapping)
                    primary = connections(pairs, assigned.primary)
                    diagnostics.append(
                        Diagnostic(
                            name=cell.name,
                            thread_index=thread.index,
                            points=len(point_owners),
                            multi_membership_turns=sum(
                                len(groups) > 1
                                for groups in assigned.overlapping.values()
                            ),
                            nearby=score(
                                near, {pair.id: predictions[pair.id] for pair in near}
                            ),
                            distant=score(
                                far, {pair.id: predictions[pair.id] for pair in far}
                            ),
                        )
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
                            seconds=preparation + assigned.seconds,
                            group_sizes=assigned.group_sizes,
                            assigned_turns=sum(
                                bool(groups) for groups in assigned.overlapping.values()
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
            click.echo(f"Scored structural development thread {thread.index}")
    frozen_rows = tuple(rows)
    summaries = tuple(
        summarize(frozen_rows, name)
        for name in (*CONTROLS, *(cell.name for cell in CELLS))
    )
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
    result = StructureReport(
        source_development_sha256=SOURCE_SHA,
        registered_cells=CELLS,
        gates=tuple(gates),
        scores=slices(
            frozen_rows,
            labels.labels,
            {ref.pair_id: ref for ref in labels.manifest.references},
        ),
        clustering=fit_costs(frozen_rows),
        diagnostics=tuple(diagnostics),
        report=Report(
            cohort_sha256=cohort_sha,
            protocol_sha256=protocol_sha,
            annotation_sha256=labels.file_sha256,
            annotation_manifest_sha256=labels.manifest_sha256,
            artifact_sha256=hashes,
            encoder_manifest_sha256=source.encoder_manifest_sha256,
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
    with output.open("xb") as destination:
        destination.write(orjson.dumps(result.model_dump(mode="json")))
    click.echo(
        f"Structure quality choice: {decision.selected}; confirmation remains sealed"
    )


if __name__ == "__main__":
    main()
