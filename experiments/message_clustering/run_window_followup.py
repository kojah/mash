"""Fixed window-point screen; source/cache reads only, no model inference."""

import hashlib
import io
import os
import sqlite3
import time
from collections import Counter
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from build_input_followup import Built
from corpus import Record
from followup_inputs import Variant
from run_input_followup import FollowupReport, Gate, choose
from run_study_development import Prediction, Report, Row, bootstrap, summarize
from run_temporal_smoothing import ANCHOR, SOURCE_SHA
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
from temporal_smoothing import links
from turn_artifact import Artifact
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase
from window_points import Representation, prepare_points


class Cell(Record):
    representation: Representation
    family: Family
    cutoff: float

    @property
    def name(self) -> str:
        return f"{self.representation}:{self.family}:{self.cutoff:.2f}"


CELLS = tuple(
    Cell(representation=representation, family=family, cutoff=cutoff)
    for representation in Representation
    for family in (Family.BIRCH, Family.AVERAGE)
    for cutoff in (0.65, 0.75)
)


class Diagnostic(Record):
    name: str
    thread_index: int
    points: int
    owning_turns: int
    maximum_points_per_turn: int
    multi_membership_turns: int
    nearby: Counts
    distant: Counts
    changed_links: int


class WindowReport(Record):
    source_development_sha256: str
    registered_cells: tuple[Cell, ...]
    report: Report
    gates: tuple[Gate, ...]
    scores: tuple[Slice, ...]
    clustering: tuple[FitCost, ...]
    diagnostics: tuple[Diagnostic, ...]
    timing_description: str = (
        "read-only cache preparation and clustering; no new embeddings"
    )
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
@click.option("--original", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--source-development", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    artifacts: Path,
    original: Path,
    source_development: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("window result exists; do not overwrite")
    raw = source_development.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SOURCE_SHA:
        raise click.ClickException("source report changed")
    source = FollowupReport.model_validate_json(raw).report
    data = cohort.read_bytes()
    cohort_sha = hashlib.sha256(data).hexdigest()
    captured = Cohort.model_validate_json(data)
    labels = load_development(annotations)
    fingerprint = (artifacts / "pooling-generation.txt").read_text().strip()
    if (
        cohort_sha != source.cohort_sha256
        or labels.manifest_sha256 != source.annotation_manifest_sha256
        or labels.file_sha256 != source.annotation_sha256
        or fingerprint != source.encoder_manifest_sha256
    ):
        raise click.ClickException("source or cache provenance mismatch")
    cache = sqlite3.connect(
        (artifacts / "vectors.sqlite").resolve().as_uri() + "?mode=ro", uri=True
    )
    manifests = sqlite3.connect(
        (original / "gold.sqlite").resolve().as_uri() + "?mode=ro", uri=True
    )
    rows = [row for row in source.rows if row.name == "chronological_4"]
    diagnostics = []
    hashes = {}
    try:
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
                    if ref.thread_index == thread.index
                    and ref.phase == Phase.DEVELOPMENT
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
                    pair
                    for pair in pairs
                    if abs(positions[pair.a] - positions[pair.b]) > 4
                )
                started = time.perf_counter()
                stem = f"{thread.index}-{Variant.CONTEXT_MEAN}"
                provenance = (artifacts / f"{stem}.json").read_bytes()
                matrix_bytes = (artifacts / f"{stem}.npz").read_bytes()
                for suffix, contents in (("json", provenance), ("npz", matrix_bytes)):
                    digest = hashlib.sha256(contents).hexdigest()
                    if source.artifact_sha256[f"{stem}.{suffix}"] != digest:
                        raise click.ClickException("frozen context artifact changed")
                    hashes[f"{stem}.{suffix}"] = digest
                built = Built.model_validate_json(provenance)
                manifest = manifests.execute(
                    "SELECT manifest FROM artifacts WHERE name=?",
                    (f"{thread.index}-{View.SEPARATE}",),
                ).fetchone()
                if not manifest:
                    raise click.ClickException("original answer manifest missing")
                artifact = Artifact.model_validate_json(manifest[0])
                if (
                    artifact.source_sha256 != cohort_sha
                    or artifact.protocol_sha256 != labels.manifest.protocol_sha256
                    or artifact.encoder_generation != fingerprint
                ):
                    raise click.ClickException("original answer manifest changed")
                hashes[f"{thread.index}-raw-manifest.json"] = hashlib.sha256(
                    manifest[0]
                ).hexdigest()
                with np.load(io.BytesIO(matrix_bytes), allow_pickle=False) as matrix:
                    control_vectors = matrix["vectors"]
                    if tuple(matrix["turn_ids"].tolist()) != owners:
                        raise click.ClickException("control owners disagree")
                common_seconds = time.perf_counter() - started
                anchor = fit(
                    control_vectors,
                    owners,
                    Candidate(
                        view=View.SEPARATE,
                        family=Family.BIRCH,
                        cutoff=0.75,
                        radius=0.60,
                    ),
                )
                expected = next(
                    row
                    for row in source.rows
                    if row.name == ANCHOR and row.thread_index == thread.index
                )
                anchor_predictions = connections(pairs, anchor.overlapping)
                if score(pairs, anchor_predictions) != expected.counts or any(
                    anchor_predictions[p.pair_id] != p.overlapping
                    for p in expected.predictions
                ):
                    raise click.ClickException(
                        "pooled anchor does not reproduce source predictions"
                    )
                rows.append(
                    expected.model_copy(
                        update={"seconds": common_seconds + anchor.seconds}
                    )
                )
                anchor_links = links(owners, anchor.overlapping)
                for representation in Representation:
                    started = time.perf_counter()
                    prepared = prepare_points(
                        built,
                        artifact,
                        thread.thread,
                        thread.turns,
                        cache,
                        fingerprint,
                        representation,
                    )
                    prep_seconds = common_seconds + time.perf_counter() - started
                    hashes[f"{thread.index}-{representation}-point-keys.json"] = (
                        hashlib.sha256(
                            orjson.dumps((prepared.owners, prepared.text_hashes))
                        ).hexdigest()
                    )
                    for cell in CELLS:
                        if cell.representation != representation:
                            continue
                        assigned = fit(
                            prepared.vectors,
                            prepared.owners,
                            Candidate(
                                view=View.SEPARATE,
                                family=cell.family,
                                cutoff=cell.cutoff,
                                radius=(0.75 if cell.cutoff == 0.65 else 0.60)
                                if cell.family == Family.BIRCH
                                else None,
                            ),
                        )
                        predictions = connections(pairs, assigned.overlapping)
                        primary = connections(pairs, assigned.primary)
                        changed = links(owners, assigned.overlapping) != anchor_links
                        np.fill_diagonal(changed, False)
                        diagnostics.append(
                            Diagnostic(
                                name=cell.name,
                                thread_index=thread.index,
                                points=len(prepared.owners),
                                owning_turns=len(owners),
                                maximum_points_per_turn=max(
                                    Counter(prepared.owners).values()
                                ),
                                multi_membership_turns=sum(
                                    len(groups) > 1
                                    for groups in assigned.overlapping.values()
                                ),
                                nearby=score(
                                    near,
                                    {pair.id: predictions[pair.id] for pair in near},
                                ),
                                distant=score(
                                    far, {pair.id: predictions[pair.id] for pair in far}
                                ),
                                changed_links=int(
                                    np.count_nonzero(np.triu(changed, 1))
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
                                seconds=prep_seconds + assigned.seconds,
                                group_sizes=assigned.group_sizes,
                                assigned_turns=sum(
                                    bool(groups)
                                    for groups in assigned.overlapping.values()
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
                click.echo(f"Scored window development thread {thread.index}")
    finally:
        cache.close()
        manifests.close()
    frozen_rows = tuple(rows)
    summaries = tuple(
        summarize(frozen_rows, name)
        for name in (ANCHOR, *(cell.name for cell in CELLS))
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
    result = WindowReport(
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
            protocol_sha256=hashlib.sha256(protocol.read_bytes()).hexdigest(),
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
        f"Window quality choice: {decision.selected}; confirmation remains sealed"
    )


if __name__ == "__main__":
    main()
