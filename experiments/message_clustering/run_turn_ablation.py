"""Eight frozen cells with full baseline parity and actual memberships."""

import hashlib
import io
import os
import time
from collections import Counter
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from ablation_evidence import (
    CELLS,
    Background,
    Evidence,
    fit,
    fuse,
    generic,
    turn_terms,
)
from ablation_inputs import validate
from build_structure_followup import StructuredArtifact
from build_turn_ablation import Artifact
from corpus import Record, Role
from followup_inputs import Variant
from local_encoder import EncoderManifest
from run_input_followup import FollowupReport, Gate, choose
from run_structure_followup import StructureReport
from run_study_development import (
    Interval,
    Prediction,
    Report,
    Row,
    bootstrap,
    summarize,
)
from run_turn_ensemble import A_SHA, B_SHA, EnsembleReport
from study_analysis import FitCost, Slice, fit_costs, slices
from study_annotations import load_development
from study_scoring import JudgedPair, absolute_gate, combine, minimum_gain, score
from temporal_smoothing import Setting, lineages, smooth
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase

BASELINE_SHA = "9835b7cd6549b07ebb4e3c55ee4190ac7294854d5a9d6dacd968b08ddfb083ce"


class Diagnostic(Record):
    name: str
    thread_index: int
    nearby: Counts
    distant: Counts


class Contrast(Record):
    label: str
    treatment: str
    control: str
    precision_delta: float
    recall_delta: float
    f1_delta: float
    long_precision_delta: float
    long_recall_delta: float
    long_f1_delta: float
    interval: Interval


class AblationReport(Record):
    baseline_sha256: str
    background_sha256: str
    source_parent_sha256: tuple[str, str]
    report: Report
    gates: tuple[Gate, ...]
    scores: tuple[Slice, ...]
    clustering: tuple[FitCost, ...]
    diagnostics: tuple[Diagnostic, ...]
    contrasts: tuple[Contrast, ...]
    generic_base_points: int
    generic_context_points: int
    confirmation_status: str = "sealed"
    timing_description: str = (
        "measured preparation and shared parent fit costs "
        "plus cell membership construction"
    )


def matrix(raw: bytes) -> tuple[np.ndarray, tuple[str, ...]]:
    with np.load(io.BytesIO(raw), allow_pickle=False) as archive:
        vectors = archive["vectors"]
        owners = tuple(archive["turn_ids"].tolist())
    if (
        vectors.shape != (len(owners), 384)
        or not np.isfinite(vectors).all()
        or not np.allclose(np.linalg.norm(vectors, axis=1), 1, atol=1e-9, rtol=0)
    ):
        raise ValueError("invalid stored clustering points")
    return vectors, owners


def primary(evidence: Evidence) -> dict[str, frozenset[int]]:
    groups: dict[str, set[int]] = {o: set() for o in evidence.owners}
    for owner, group in zip(evidence.owners, evidence.partition, strict=True):
        if group >= 0:
            groups[owner].add(group)
    return {owner: frozenset(values) for owner, values in groups.items()}


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--baseline", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--pooled-report", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--structural-report", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--pooled-artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--structural-artifacts",
    type=click.Path(path_type=Path, exists=True),
    required=True,
)
@click.option(
    "--conditioned-artifacts",
    type=click.Path(path_type=Path, exists=True),
    required=True,
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    baseline: Path,
    pooled_report: Path,
    structural_report: Path,
    pooled_artifacts: Path,
    structural_artifacts: Path,
    conditioned_artifacts: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("ablation result exists; do not overwrite")
    baseline_raw, a_raw, b_raw = (
        baseline.read_bytes(),
        pooled_report.read_bytes(),
        structural_report.read_bytes(),
    )
    if tuple(hashlib.sha256(x).hexdigest() for x in (baseline_raw, a_raw, b_raw)) != (
        BASELINE_SHA,
        A_SHA,
        B_SHA,
    ):
        raise click.ClickException("frozen baseline or parent reports changed")
    old = EnsembleReport.model_validate_json(baseline_raw).report
    a_parent = FollowupReport.model_validate_json(a_raw).report
    b_parent = StructureReport.model_validate_json(b_raw).report
    raw = cohort.read_bytes()
    cohort_sha = hashlib.sha256(raw).hexdigest()
    captured = Cohort.model_validate_json(raw)
    labels = load_development(annotations)
    for parent in (old, a_parent, b_parent):
        if (
            parent.cohort_sha256 != cohort_sha
            or parent.annotation_manifest_sha256 != labels.manifest_sha256
            or parent.annotation_sha256 != labels.file_sha256
        ):
            raise click.ClickException("cohort or labels differ from frozen baseline")
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    encoder_raw = (structural_artifacts / "encoder.json").read_bytes()
    if (
        hashlib.sha256(encoder_raw).hexdigest() != a_parent.encoder_manifest_sha256
        or a_parent.encoder_manifest_sha256 != b_parent.encoder_manifest_sha256
    ):
        raise click.ClickException("encoder manifest changed")
    fingerprint = hashlib.sha256(
        orjson.dumps(EncoderManifest.model_validate_json(encoder_raw).model_dump())
    ).hexdigest()
    background_raw = (conditioned_artifacts / "background.json").read_bytes()
    background = Background.model_validate_json(background_raw)
    df: Counter[str] = Counter()
    documents = 0
    for entry in captured.entries:
        if entry.phase == Phase.DEVELOPMENT:
            for turn in entry.captured.turns:
                if turn.prompts:
                    df.update(turn_terms(entry.captured.thread, turn))
                    documents += 1
    if background != Background(
        documents=documents, document_frequency=dict(sorted(df.items()))
    ):
        raise click.ClickException("label-free background vocabulary changed")
    rows = [r for r in a_parent.rows if r.name == "chronological_4"]
    diagnostics = []
    hashes = {}
    generic_base = generic_context = 0
    with threadpool_limits(limits=1):
        for entry in captured.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            index = entry.captured.index
            owners = tuple(t.id for t in entry.captured.turns if t.prompts)
            positions = {owner: i for i, owner in enumerate(owners)}
            refs = tuple(
                r
                for r in labels.manifest.references
                if r.phase == Phase.DEVELOPMENT and r.thread_index == index
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
            started = time.perf_counter()
            pooled_name = f"{index}-{Variant.CONTEXT_MEAN}.npz"
            pooled_raw = (pooled_artifacts / pooled_name).read_bytes()
            if (
                hashlib.sha256(pooled_raw).hexdigest()
                != a_parent.artifact_sha256[pooled_name]
            ):
                raise click.ClickException("pooled vectors changed")
            hashes[f"pooled/{pooled_name}"] = hashlib.sha256(pooled_raw).hexdigest()
            pooled, pooled_owners = matrix(pooled_raw)
            if pooled_owners != owners:
                raise click.ClickException("pooled owners changed")
            pooled = smooth(
                pooled,
                owners,
                lineages(entry.captured.thread, entry.captured.turns),
                Setting(alpha=0.10, window=4),
            )
            a = fit(pooled, owners)
            a_seconds = time.perf_counter() - started
            evidence = {}
            penalty_masks = {}
            costs = {}
            for context in (False, True):
                started = time.perf_counter()
                if context:
                    name = f"{index}.json"
                    provenance = (conditioned_artifacts / name).read_bytes()
                    built = Artifact.model_validate_json(provenance)
                    vector_raw = (conditioned_artifacts / f"{index}.npz").read_bytes()
                    if (
                        built.thread_index != index
                        or built.protocol_sha256 != protocol_sha
                        or built.cohort_sha256 != cohort_sha
                        or built.encoder_generation != fingerprint
                        or built.matrix_sha256 != hashlib.sha256(vector_raw).hexdigest()
                        or built.source_sha256
                        != b_parent.artifact_sha256[f"{index}-structure_heading.json"]
                    ):
                        raise click.ClickException(
                            "conditioned input/vector provenance changed"
                        )
                    validate(
                        built.units, owners, entry.captured.thread, entry.captured.turns
                    )
                    bodies = tuple(u.body for u in built.units)
                    namespace = "conditioned"
                else:
                    name = f"{index}-structure_heading.json"
                    provenance = (structural_artifacts / name).read_bytes()
                    source = StructuredArtifact.model_validate_json(provenance)
                    vector_raw = (
                        structural_artifacts / f"{index}-structure_heading.npz"
                    ).read_bytes()
                    if (
                        hashlib.sha256(provenance).hexdigest()
                        != b_parent.artifact_sha256[name]
                        or source.matrix_sha256
                        != hashlib.sha256(vector_raw).hexdigest()
                        or source.turn_ids != owners
                    ):
                        raise click.ClickException("original chunk provenance changed")
                    bodies = tuple(u.input for u in source.units)
                    namespace = "structural"
                hashes[f"{namespace}/{name}"] = hashlib.sha256(provenance).hexdigest()
                hashes[f"{namespace}/{index}.npz"] = hashlib.sha256(
                    vector_raw
                ).hexdigest()
                vectors, point_owners = matrix(vector_raw)
                if point_owners != tuple(i.turn_id for i in bodies) or set(
                    point_owners
                ) != set(owners):
                    raise click.ClickException("chunk owners changed")
                evidence[context] = fit(vectors, point_owners)
                masks = []
                for item in bodies:
                    text = "\n".join(
                        item.content.text[location.start : location.end]
                        for location in item.content.locations
                        if entry.captured.thread.messages[location.source.source].role
                        == Role.ASSISTANT
                    )
                    masks.append(
                        item.role == Role.ASSISTANT and generic(text, background)
                    )
                penalty_masks[context] = tuple(0.05 if flag else 0.0 for flag in masks)
                if context:
                    generic_context += sum(masks)
                else:
                    generic_base += sum(masks)
                costs[context] = time.perf_counter() - started
            pa = primary(a)
            near = tuple(p for p in pairs if abs(positions[p.a] - positions[p.b]) <= 4)
            far = tuple(p for p in pairs if abs(positions[p.a] - positions[p.b]) > 4)
            for cell in CELLS:
                started = time.perf_counter()
                b = evidence[cell.context]
                penalties = (
                    penalty_masks[cell.context]
                    if cell.generic
                    else (0.0,) * len(b.owners)
                )
                members = fuse(a, b, penalties, cell.fusion)
                predicted = {p.id: bool(members[p.a] & members[p.b]) for p in pairs}
                pb = primary(b)
                primary_predicted = {
                    p.id: bool(pa[p.a] & pa[p.b]) or bool(pb[p.a] & pb[p.b])
                    for p in pairs
                }
                counts = score(pairs, predicted)
                primary_counts = score(pairs, primary_predicted)
                if cell.name == "c0f0g0":
                    prior = next(
                        r
                        for r in old.rows
                        if r.thread_index == index and r.name == "union"
                    )
                    if (
                        counts != prior.counts
                        or primary_counts != prior.primary_counts
                        or any(
                            predicted[p.pair_id] != p.overlapping
                            or primary_predicted[p.pair_id] != p.primary
                            for p in prior.predictions
                        )
                    ):
                        raise click.ClickException(
                            "full baseline does not reproduce frozen predictions"
                        )
                group_sizes = Counter(g for values in members.values() for g in values)
                seconds = (
                    a_seconds + costs[cell.context] + time.perf_counter() - started
                )
                rows.append(
                    Row(
                        name=cell.name,
                        thread_index=index,
                        provider=entry.captured.thread.provider.value,
                        node=entry.node,
                        prompted_turns=len(owners),
                        counts=counts,
                        primary_counts=primary_counts,
                        seconds=seconds,
                        group_sizes=tuple(group_sizes[g] for g in sorted(group_sizes)),
                        assigned_turns=sum(bool(v) for v in members.values()),
                        predictions=tuple(
                            Prediction(
                                pair_id=r.pair_id,
                                stratum=r.stratum,
                                batch=r.batch,
                                overlapping=predicted[r.pair_id],
                                primary=primary_predicted[r.pair_id],
                            )
                            for r in refs
                        ),
                    )
                )
                diagnostics.append(
                    Diagnostic(
                        name=cell.name,
                        thread_index=index,
                        nearby=score(near, {p.id: predicted[p.id] for p in near}),
                        distant=score(far, {p.id: predicted[p.id] for p in far}),
                    )
                )
    frozen = tuple(rows)
    summaries = tuple(summarize(frozen, cell.name) for cell in CELLS)
    baseline_summary = summaries[0]
    chronological = summarize(frozen, "chronological_4")
    decision = choose(summaries, frozen, chronological)

    def ordered(name: str):
        return tuple(
            r.counts
            for r in sorted(
                (r for r in frozen if r.name == name), key=lambda r: r.thread_index
            )
        )

    long = {
        cell.name: combine(
            tuple(
                r.counts
                for r in frozen
                if r.name == cell.name and r.prompted_turns >= 8
            )
        )
        for cell in CELLS
    }
    by_name = {s.name: s for s in summaries}
    gates = tuple(
        Gate(
            name=s.name,
            overall=s.counts,
            long_history=long[s.name],
            quality_eligible=absolute_gate(s.counts)
            and absolute_gate(long[s.name])
            and minimum_gain(s.counts, chronological.counts),
            interval=bootstrap(ordered(s.name), ordered(baseline_summary.name)),
        )
        for s in summaries
    )
    contrasts = []
    for label, treatment, control in (
        ("context alone", "c1f0g0", "c0f0g0"),
        ("fusion alone", "c0f1g0", "c0f0g0"),
        ("generic alone", "c0f0g1", "c0f0g0"),
        ("context leave-out", "c1f1g1", "c0f1g1"),
        ("fusion leave-out", "c1f1g1", "c1f0g1"),
        ("generic leave-out", "c1f1g1", "c1f1g0"),
    ):
        left, right = by_name[treatment].counts, by_name[control].counts
        ll, lr = long[treatment], long[control]
        contrasts.append(
            Contrast(
                label=label,
                treatment=treatment,
                control=control,
                precision_delta=left.precision - right.precision,
                recall_delta=left.recall - right.recall,
                f1_delta=left.f1 - right.f1,
                long_precision_delta=ll.precision - lr.precision,
                long_recall_delta=ll.recall - lr.recall,
                long_f1_delta=ll.f1 - lr.f1,
                interval=bootstrap(ordered(treatment), ordered(control)),
            )
        )
    result = AblationReport(
        baseline_sha256=BASELINE_SHA,
        background_sha256=hashlib.sha256(background_raw).hexdigest(),
        source_parent_sha256=(A_SHA, B_SHA),
        gates=gates,
        contrasts=tuple(contrasts),
        scores=slices(
            frozen, labels.labels, {r.pair_id: r for r in labels.manifest.references}
        ),
        clustering=fit_costs(frozen),
        diagnostics=tuple(diagnostics),
        generic_base_points=generic_base,
        generic_context_points=generic_context,
        report=Report(
            cohort_sha256=cohort_sha,
            protocol_sha256=protocol_sha,
            annotation_sha256=labels.file_sha256,
            annotation_manifest_sha256=labels.manifest_sha256,
            artifact_sha256=hashes,
            encoder_manifest_sha256=a_parent.encoder_manifest_sha256,
            agreement=labels.agreement,
            candidates=summaries,
            baselines=(baseline_summary, chronological),
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
        f"Eight ablations complete; quality choice {decision.selected}; "
        "confirmation sealed"
    )


if __name__ == "__main__":
    main()
