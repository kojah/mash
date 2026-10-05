"""Execute the registered five-cell temporal comparison on cached DEV vectors."""

import hashlib
import io
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from corpus import Record
from followup_inputs import Variant
from run_input_followup import FollowupReport, Gate, choose
from run_study_development import Prediction, Report, Row, bootstrap, summarize
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
from temporal_smoothing import SETTINGS, lineages, links, smooth
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase

SOURCE_SHA = "d95bb0cb317a40fcd107db4ac6ff4074f66c76a21b87301d22a802f1f2b6bf1e"
ANCHOR = "context_whole:birch:0.60:0.75:centroid"


class Diagnostic(Record):
    name: str
    thread_index: int
    nearby: Counts
    distant: Counts
    changed_links: int
    changed_turns: int
    cosine_displacements: tuple[float, ...]


class TemporalReport(Record):
    source_development_sha256: str
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
        raise click.ClickException("temporal result exists; do not overwrite")
    raw = source_development.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SOURCE_SHA:
        raise click.ClickException("anchor development report changed")
    source = FollowupReport.model_validate_json(raw).report
    data = cohort.read_bytes()
    cohort_sha = hashlib.sha256(data).hexdigest()
    captured = Cohort.model_validate_json(data)
    labels = load_development(annotations)
    if (
        cohort_sha != source.cohort_sha256
        or labels.manifest_sha256 != source.annotation_manifest_sha256
        or labels.file_sha256 != source.annotation_sha256
    ):
        raise click.ClickException("cohort or annotations changed")
    rows = [row for row in source.rows if row.name == "chronological_4"]
    diagnostics = []
    matrix_hashes = {}
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
            name = f"{thread.index}-{Variant.CONTEXT_MEAN}.npz"
            matrix_bytes = (artifacts / name).read_bytes()
            digest = hashlib.sha256(matrix_bytes).hexdigest()
            if source.artifact_sha256[name] != digest:
                raise click.ClickException("frozen anchor vectors changed")
            matrix_hashes[name] = digest
            with np.load(io.BytesIO(matrix_bytes), allow_pickle=False) as matrix:
                vectors = matrix["vectors"]
                if tuple(matrix["turn_ids"].tolist()) != owners:
                    raise click.ClickException("anchor turn identities differ")
            started = time.perf_counter()
            ancestors = lineages(thread.thread, thread.turns)
            lineage_seconds = time.perf_counter() - started
            original_links = None
            for setting in SETTINGS:
                started = time.perf_counter()
                points = smooth(vectors, owners, ancestors, setting)
                assigned = fit(
                    points,
                    owners,
                    Candidate(
                        view=View.SEPARATE,
                        family=Family.BIRCH,
                        cutoff=0.75,
                        radius=0.60,
                    ),
                )
                seconds = time.perf_counter() - started + lineage_seconds
                predictions = connections(pairs, assigned.overlapping)
                primary = connections(pairs, assigned.primary)
                counts = score(pairs, predictions)
                if setting.alpha == 0:
                    previous = next(
                        row
                        for row in source.rows
                        if row.thread_index == thread.index and row.name == ANCHOR
                    )
                    if (
                        counts != previous.counts
                        or assigned.group_sizes != previous.group_sizes
                        or any(
                            predictions[p.pair_id] != p.overlapping
                            for p in previous.predictions
                        )
                    ):
                        raise click.ClickException(
                            "zero-smoothing control differs from anchor"
                        )
                    original_links = links(owners, assigned.overlapping)
                if original_links is None:
                    raise click.ClickException(
                        "control must precede transformed settings"
                    )
                changed = links(owners, assigned.overlapping) != original_links
                np.fill_diagonal(changed, False)
                near = tuple(
                    pair
                    for pair in pairs
                    if abs(positions[pair.a] - positions[pair.b]) <= 4
                )
                far = tuple(
                    pair
                    for pair in pairs
                    if abs(positions[pair.a] - positions[pair.b]) > 4
                )
                diagnostics.append(
                    Diagnostic(
                        name=setting.name,
                        thread_index=thread.index,
                        nearby=score(
                            near, {pair.id: predictions[pair.id] for pair in near}
                        ),
                        distant=score(
                            far, {pair.id: predictions[pair.id] for pair in far}
                        ),
                        changed_links=int(np.count_nonzero(np.triu(changed, 1))),
                        changed_turns=int(np.count_nonzero(changed.any(axis=1))),
                        cosine_displacements=tuple(
                            float(value)
                            for value in np.clip(
                                1 - np.sum(vectors * points, axis=1), 0, 2
                            )
                        ),
                    )
                )
                rows.append(
                    Row(
                        name=setting.name,
                        thread_index=thread.index,
                        provider=thread.thread.provider.value,
                        node=entry.node,
                        prompted_turns=len(owners),
                        counts=counts,
                        primary_counts=score(pairs, primary),
                        seconds=seconds,
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
            click.echo(f"Scored temporal development thread {thread.index}")
    frozen_rows = tuple(rows)
    summaries = tuple(summarize(frozen_rows, setting.name) for setting in SETTINGS)
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
    result = TemporalReport(
        source_development_sha256=SOURCE_SHA,
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
            protocol_sha256=hashlib.sha256(protocol.read_bytes()).hexdigest(),
            annotation_sha256=labels.file_sha256,
            annotation_manifest_sha256=labels.manifest_sha256,
            artifact_sha256=matrix_hashes,
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
        f"Temporal quality choice: {decision.selected}; confirmation remains sealed"
    )


if __name__ == "__main__":
    main()
