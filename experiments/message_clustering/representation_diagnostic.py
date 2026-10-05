"""Development-only scalar separability; never select a clustering algorithm."""

import hashlib
import io
import os
from pathlib import Path

import click
import numpy as np
import orjson
from sklearn.metrics import roc_auc_score
from threadpoolctl import threadpool_limits

from blind_turn_pairs import Verdict
from clustering import VectorMatrix
from corpus import Record
from run_input_followup import FollowupReport
from run_structure_followup import StructureReport
from run_turn_ensemble import A_SHA, B_SHA
from study_annotations import load_development
from turn_study_corpus import Cohort, Phase


class Observation(Record):
    view: str
    thread_index: int
    pair_id: str
    verdict: Verdict
    turns: int
    distance: int
    similarity: float
    comparisons: int


class Summary(Record):
    view: str
    scope: str
    positives: int
    negatives: int
    auc: float | None
    positive_median: float | None
    negative_median: float | None
    positive_comparisons_median: float | None
    negative_comparisons_median: float | None
    optimistic_recall_at_precision_85: float


class Diagnostic(Record):
    parent_a_sha256: str
    parent_b_sha256: str
    cohort_sha256: str
    protocol_sha256: str
    annotation_manifest_sha256: str
    annotation_sha256: dict[str, str]
    summaries: tuple[Summary, ...]
    observations: tuple[Observation, ...]
    confirmation_status: str = "sealed"
    decision: None = None


def normalize(vectors: VectorMatrix) -> VectorMatrix:
    norms = np.linalg.norm(vectors, axis=1)
    if not np.isfinite(vectors).all() or np.any(norms <= 1e-12):
        raise ValueError("diagnostic requires finite nonzero vectors")
    return vectors / norms[:, None]


def summarize(items: tuple[Observation, ...], view: str, scope: str) -> Summary:
    selected = tuple(
        o
        for o in items
        if o.view == view
        and o.verdict != Verdict.AMBIGUOUS
        and (
            scope == "all"
            or scope == "short"
            and o.turns < 8
            or scope == "long"
            and o.turns >= 8
            or scope == "nearby"
            and o.distance <= 4
            or scope == "distant"
            and o.distance > 4
        )
    )
    positives = tuple(o for o in selected if o.verdict == Verdict.RELATED)
    negatives = tuple(o for o in selected if o.verdict == Verdict.UNRELATED)
    # Include whole ties: a threshold cannot select a favourable subset of a tie.
    buckets: dict[float, list[bool]] = {}
    for o in selected:
        buckets.setdefault(o.similarity, []).append(o.verdict == Verdict.RELATED)
    tp = fp = 0
    best = 0.0
    for _, verdicts in sorted(buckets.items(), reverse=True):
        tp += sum(verdicts)
        fp += len(verdicts) - sum(verdicts)
        if positives and 100 * tp >= 85 * (tp + fp):
            best = max(best, tp / len(positives))
    return Summary(
        view=view,
        scope=scope,
        positives=len(positives),
        negatives=len(negatives),
        auc=float(
            roc_auc_score(
                [o.verdict == Verdict.RELATED for o in selected],
                [o.similarity for o in selected],
            )
        )
        if positives and negatives
        else None,
        positive_median=float(np.median([o.similarity for o in positives]))
        if positives
        else None,
        negative_median=float(np.median([o.similarity for o in negatives]))
        if negatives
        else None,
        positive_comparisons_median=float(np.median([o.comparisons for o in positives]))
        if positives
        else None,
        negative_comparisons_median=float(np.median([o.comparisons for o in negatives]))
        if negatives
        else None,
        optimistic_recall_at_precision_85=best,
    )


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--parent-a", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--parent-b", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--pooled-artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--structural-artifacts",
    type=click.Path(path_type=Path, exists=True),
    required=True,
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    annotations: Path,
    parent_a: Path,
    parent_b: Path,
    pooled_artifacts: Path,
    structural_artifacts: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException("diagnostic exists; do not overwrite")
    a_bytes, b_bytes = parent_a.read_bytes(), parent_b.read_bytes()
    if (
        hashlib.sha256(a_bytes).hexdigest() != A_SHA
        or hashlib.sha256(b_bytes).hexdigest() != B_SHA
    ):
        raise click.ClickException("parent reports changed")
    a = FollowupReport.model_validate_json(a_bytes).report
    b = StructureReport.model_validate_json(b_bytes).report
    raw = cohort.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    captured = Cohort.model_validate_json(raw)
    labels = load_development(annotations)
    for parent in (a, b):
        if (
            parent.cohort_sha256 != digest
            or parent.annotation_manifest_sha256 != labels.manifest_sha256
            or parent.annotation_sha256 != labels.file_sha256
        ):
            raise click.ClickException("cohort or annotations changed")
    observations = []
    with threadpool_limits(limits=1):
        for entry in captured.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            index = entry.captured.index
            turns = tuple(t.id for t in entry.captured.turns if t.prompts)
            positions = {owner: i for i, owner in enumerate(turns)}
            refs = tuple(
                r
                for r in labels.manifest.references
                if r.thread_index == index and r.phase == Phase.DEVELOPMENT
            )
            for view in ("raw", "context_whole", "structure_heading"):
                structural = view == "structure_heading"
                directory = structural_artifacts if structural else pooled_artifacts
                parent = b if structural else a
                name = f"{index}-{view}.npz"
                data = (directory / name).read_bytes()
                if hashlib.sha256(data).hexdigest() != parent.artifact_sha256[name]:
                    raise click.ClickException("matrix hash changed")
                with np.load(io.BytesIO(data), allow_pickle=False) as archive:
                    vectors = normalize(archive["vectors"])
                    owners = tuple(archive["turn_ids"].tolist())
                if (
                    vectors.shape != (len(owners), 384)
                    or set(owners) != set(turns)
                    or not structural
                    and owners != turns
                ):
                    raise click.ClickException("matrix identities differ")
                for ref in refs:
                    left = vectors[
                        [i for i, owner in enumerate(owners) if owner == ref.a_turn_id]
                    ]
                    right = vectors[
                        [i for i, owner in enumerate(owners) if owner == ref.b_turn_id]
                    ]
                    observations.append(
                        Observation(
                            view=view,
                            thread_index=index,
                            pair_id=ref.pair_id,
                            verdict=labels.labels[ref.pair_id].verdict,
                            turns=len(turns),
                            distance=abs(
                                positions[ref.a_turn_id] - positions[ref.b_turn_id]
                            ),
                            similarity=float(np.max(left @ right.T)),
                            comparisons=len(left) * len(right),
                        )
                    )
    frozen = tuple(observations)
    result = Diagnostic(
        parent_a_sha256=A_SHA,
        parent_b_sha256=B_SHA,
        cohort_sha256=digest,
        protocol_sha256=hashlib.sha256(protocol.read_bytes()).hexdigest(),
        annotation_manifest_sha256=labels.manifest_sha256,
        annotation_sha256=labels.file_sha256,
        observations=frozen,
        summaries=tuple(
            summarize(frozen, view, scope)
            for view in ("raw", "context_whole", "structure_heading")
            for scope in ("all", "short", "long", "nearby", "distant")
        ),
    )
    with output.open("xb") as stream:
        stream.write(
            orjson.dumps(result.model_dump(mode="json"), option=orjson.OPT_INDENT_2)
        )
    click.echo("Representation diagnostic complete; no candidate selected")


if __name__ == "__main__":
    main()
