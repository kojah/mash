"""Single registered selective treatment against two immutable controls."""

import hashlib
import io
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from analyze_input_followup import validate_sources
from build_input_followup import Built
from followup_inputs import Variant
from run_input_followup import FollowupReport, Gate, choose
from run_study_development import (
    Interval,
    Prediction,
    Report,
    Row,
    bootstrap,
    summarize,
)
from run_temporal_smoothing import ANCHOR, SOURCE_SHA, Diagnostic, TemporalReport
from selective_smoothing import NAME, Eligibility, eligibility, refine
from study_analysis import fit_costs, slices
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
from temporal_smoothing import lineages, links
from turn_study_corpus import Cohort, Phase

TEMPORAL_SHA = "d4ab356b33f2fbcd4905fc61fcceb2cb1d5793d1735eb1d088fe69ca0fedcb48"
CONTROLS = ("temporal:0.00:w0", "temporal:0.10:w4")


class TargetScope(Eligibility):
    thread_index: int
    moved: bool


class Gain(Interval):
    control: str


class SelectiveReport(TemporalReport):
    source_temporal_sha256: str
    targets: tuple[TargetScope, ...]
    gains: tuple[Gain, ...]
    timing_note: str = (
        "controls retain historical timing; treatment includes new input "
        "preparation, smoothing and fit"
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
@click.option("--source", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--temporal", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    artifacts: Path,
    source: Path,
    temporal: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("selective result exists; do not overwrite")
    raw, old_raw = source.read_bytes(), temporal.read_bytes()
    if (
        hashlib.sha256(raw).hexdigest() != SOURCE_SHA
        or hashlib.sha256(old_raw).hexdigest() != TEMPORAL_SHA
    ):
        raise click.ClickException("frozen controls changed")
    parent = FollowupReport.model_validate_json(raw).report
    control = TemporalReport.model_validate_json(old_raw)
    captured_raw = cohort.read_bytes()
    cohort_sha = hashlib.sha256(captured_raw).hexdigest()
    captured = Cohort.model_validate_json(captured_raw)
    labels = load_development(annotations)
    for report in (parent, control.report):
        if (
            report.cohort_sha256 != cohort_sha
            or report.annotation_manifest_sha256 != labels.manifest_sha256
            or report.annotation_sha256 != labels.file_sha256
        ):
            raise click.ClickException("cohort/labels differ from controls")
    rows = [
        r
        for r in control.report.rows
        if r.name in CONTROLS or r.name == "chronological_4"
    ]
    diagnostics = [d for d in control.diagnostics if d.name in CONTROLS]
    hashes = {}
    targets = []
    candidate = Candidate(
        view=View.SEPARATE, family=Family.BIRCH, cutoff=0.75, radius=0.60
    )
    with threadpool_limits(limits=1):
        for entry in captured.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            started = time.perf_counter()
            thread = entry.captured
            owners = tuple(t.id for t in thread.turns if t.prompts)
            positions = {owner: i for i, owner in enumerate(owners)}
            matrix_name = f"{thread.index}-{Variant.CONTEXT_MEAN}.npz"
            matrix_raw = (artifacts / matrix_name).read_bytes()
            hashes[matrix_name] = hashlib.sha256(matrix_raw).hexdigest()
            input_name = f"{thread.index}-{Variant.LEAN}.json"
            input_raw = (artifacts / input_name).read_bytes()
            hashes[input_name] = hashlib.sha256(input_raw).hexdigest()
            if any(
                parent.artifact_sha256[n] != hashes[n]
                for n in (matrix_name, input_name)
            ):
                raise click.ClickException("cached input/vector provenance changed")
            built = Built.model_validate_json(input_raw)
            if (
                built.turn_ids != owners
                or built.thread_index != thread.index
                or built.phase != Phase.DEVELOPMENT
            ):
                raise click.ClickException("lean turn identities differ")
            validate_sources(built, thread.thread, thread.turns)
            shortness = eligibility(built)
            with np.load(io.BytesIO(matrix_raw), allow_pickle=False) as archive:
                vectors = archive["vectors"]
                if tuple(archive["turn_ids"].tolist()) != owners:
                    raise click.ClickException("vector turn identities differ")
            ancestry = lineages(thread.thread, thread.turns)
            points = refine(
                vectors,
                owners,
                ancestry,
                frozenset(t.turn_id for t in shortness if t.eligible),
            )
            assigned = fit(points, owners, candidate)
            seconds = time.perf_counter() - started
            refs = tuple(
                r
                for r in labels.manifest.references
                if r.phase == Phase.DEVELOPMENT and r.thread_index == thread.index
            )
            pairs = tuple(
                JudgedPair(
                    r.pair_id,
                    r.a_turn_id,
                    r.b_turn_id,
                    labels.labels[r.pair_id].verdict,
                )
                for r in refs
            )
            prediction = connections(pairs, assigned.overlapping)
            primary = connections(pairs, assigned.primary)
            original = fit(vectors, owners, candidate)
            anchor = next(
                r
                for r in parent.rows
                if r.thread_index == thread.index and r.name == ANCHOR
            )
            if score(
                pairs, connections(pairs, original.overlapping)
            ) != anchor.counts or any(
                connections(pairs, original.overlapping)[p.pair_id] != p.overlapping
                for p in anchor.predictions
            ):
                raise click.ClickException(
                    "refitted anchor differs from frozen control"
                )
            changed = links(owners, assigned.overlapping) != links(
                owners, original.overlapping
            )
            np.fill_diagonal(changed, False)
            displacement = np.clip(1 - np.sum(vectors * points, axis=1), 0, 2)
            targets.extend(
                TargetScope(
                    **t.model_dump(),
                    thread_index=thread.index,
                    moved=not np.array_equal(vectors[i], points[i]),
                )
                for i, t in enumerate(shortness)
            )
            near = tuple(p for p in pairs if abs(positions[p.a] - positions[p.b]) <= 4)
            far = tuple(p for p in pairs if abs(positions[p.a] - positions[p.b]) > 4)
            diagnostics.append(
                Diagnostic(
                    name=NAME,
                    thread_index=thread.index,
                    nearby=score(near, {p.id: prediction[p.id] for p in near}),
                    distant=score(far, {p.id: prediction[p.id] for p in far}),
                    changed_links=int(np.count_nonzero(np.triu(changed, 1))),
                    changed_turns=int(np.count_nonzero(changed.any(axis=1))),
                    cosine_displacements=tuple(float(x) for x in displacement),
                )
            )
            rows.append(
                Row(
                    name=NAME,
                    thread_index=thread.index,
                    provider=thread.thread.provider.value,
                    node=entry.node,
                    prompted_turns=len(owners),
                    counts=score(pairs, prediction),
                    primary_counts=score(pairs, primary),
                    seconds=seconds,
                    group_sizes=assigned.group_sizes,
                    assigned_turns=sum(bool(g) for g in assigned.overlapping.values()),
                    predictions=tuple(
                        Prediction(
                            pair_id=r.pair_id,
                            stratum=r.stratum,
                            batch=r.batch,
                            overlapping=prediction[r.pair_id],
                            primary=primary[r.pair_id],
                        )
                        for r in refs
                    ),
                )
            )
    frozen = tuple(rows)
    summaries = tuple(summarize(frozen, name) for name in (*CONTROLS, NAME))
    baseline = summarize(frozen, "chronological_4")
    decision = choose(summaries, frozen, baseline)

    def ordered_counts(name: str):
        return tuple(
            r.counts
            for r in sorted(
                (r for r in frozen if r.name == name), key=lambda r: r.thread_index
            )
        )

    gates = []
    for summary in summaries:
        long = combine(
            tuple(
                r.counts
                for r in frozen
                if r.name == summary.name and r.prompted_turns >= 8
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
                    ordered_counts(summary.name), ordered_counts(baseline.name)
                ),
            )
        )
    result = SelectiveReport(
        source_development_sha256=SOURCE_SHA,
        source_temporal_sha256=TEMPORAL_SHA,
        targets=tuple(targets),
        gates=tuple(gates),
        gains=tuple(
            Gain(
                **bootstrap(ordered_counts(NAME), ordered_counts(name)).model_dump(),
                control=name,
            )
            for name in CONTROLS
        ),
        scores=slices(
            frozen, labels.labels, {r.pair_id: r for r in labels.manifest.references}
        ),
        clustering=fit_costs(frozen),
        diagnostics=tuple(diagnostics),
        report=Report(
            cohort_sha256=cohort_sha,
            protocol_sha256=hashlib.sha256(protocol.read_bytes()).hexdigest(),
            annotation_sha256=labels.file_sha256,
            annotation_manifest_sha256=labels.manifest_sha256,
            artifact_sha256=hashes,
            encoder_manifest_sha256=parent.encoder_manifest_sha256,
            agreement=labels.agreement,
            candidates=summaries,
            baselines=(baseline,),
            decision=decision,
            rows=frozen,
            selected_interval=next(
                (g.interval for g in gates if g.name == decision.selected), None
            ),
        ),
    )
    with output.open("xb") as stream:
        stream.write(orjson.dumps(result.model_dump(mode="json")))
    click.echo(
        f"Selective quality choice: {decision.selected}; confirmation remains sealed"
    )


if __name__ == "__main__":
    main()
