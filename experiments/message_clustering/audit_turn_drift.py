"""Development-only counterfactual: hold embeddings fixed, remove later turns."""

import hashlib
import io
import os
from enum import StrEnum
from pathlib import Path

import click
import numpy as np
import orjson
from sklearn.metrics import roc_auc_score
from threadpoolctl import threadpool_limits

from blind_turn_pairs import Verdict
from clustering import VectorMatrix
from corpus import Record
from run_study_development import Report
from study_annotations import load_development
from study_scoring import CANDIDATES, Candidate, JudgedPair, connections, fit

DIAGNOSTICS = (
    "separate_view_pooling:birch:0.65",
    "joint_truncation:birch:0.75",
)


class Length(StrEnum):
    ALL = "all"
    SHORT = "2-7"
    LONG = "8+"


class Observation(Record):
    name: str
    thread_index: int
    prompted_turns: int
    pair_id: str
    verdict: Verdict
    cosine: float
    prefix_turns: int
    full: bool
    prefix: bool
    full_primary: bool
    prefix_primary: bool


def prefix_prediction(
    vectors: VectorMatrix,
    owners: tuple[str, ...],
    pair: JudgedPair,
    candidate: Candidate,
) -> tuple[bool, bool, int]:
    """One point per turn; retain arrival order and never recompute embeddings."""
    if len(set(owners)) != len(owners):
        raise ValueError("prefix audit requires exactly one point per turn")
    if pair.a == pair.b:
        raise ValueError("prefix audit needs two distinct turns")
    stop = max(owners.index(pair.a), owners.index(pair.b)) + 1
    assigned = fit(vectors[:stop], owners[:stop], candidate)
    return (
        connections((pair,), assigned.overlapping)[pair.id],
        connections((pair,), assigned.primary)[pair.id],
        stop,
    )


class Drift(Record):
    name: str
    length: Length
    pairs: int
    with_later_turns: int
    changed: int
    corrected_by_removing_later: int
    worsened_by_removing_later: int
    primary_changed: int
    pair_cosine_auc: float | None


def summarize(
    observations: tuple[Observation, ...], name: str, length: Length
) -> Drift:
    selected = tuple(
        o
        for o in observations
        if o.name == name
        and o.verdict != Verdict.AMBIGUOUS
        and (
            length == Length.ALL
            or (Length.LONG if o.prompted_turns >= 8 else Length.SHORT) == length
        )
    )
    later = tuple(o for o in selected if o.prefix_turns < o.prompted_turns)
    labels = tuple(o.verdict == Verdict.RELATED for o in selected)
    return Drift(
        name=name,
        length=length,
        pairs=len(selected),
        with_later_turns=len(later),
        changed=sum(o.full != o.prefix for o in later),
        corrected_by_removing_later=sum(
            o.full != (o.verdict == Verdict.RELATED)
            and o.prefix == (o.verdict == Verdict.RELATED)
            for o in later
        ),
        worsened_by_removing_later=sum(
            o.full == (o.verdict == Verdict.RELATED)
            and o.prefix != (o.verdict == Verdict.RELATED)
            for o in later
        ),
        primary_changed=sum(o.full_primary != o.prefix_primary for o in later),
        pair_cosine_auc=float(roc_auc_score(labels, [o.cosine for o in selected]))
        if len(set(labels)) == 2
        else None,
    )


class Audit(Record):
    development_sha256: str
    summaries: tuple[Drift, ...]
    observations: tuple[Observation, ...]


@click.command()
@click.option(
    "--development", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(development: Path, annotations: Path, artifacts: Path, output: Path) -> None:
    os.umask(0o077)
    raw = development.read_bytes()
    report = Report.model_validate_json(raw)
    labels = load_development(annotations)
    if (
        labels.manifest_sha256 != report.annotation_manifest_sha256
        or labels.file_sha256 != report.annotation_sha256
    ):
        raise click.ClickException("annotations changed after development scoring")
    references = {
        ref.pair_id: ref
        for ref in labels.manifest.references
        if ref.pair_id in labels.labels
    }
    observations: list[Observation] = []
    with threadpool_limits(limits=1):
        for row in report.rows:
            if row.name not in DIAGNOSTICS:
                continue
            candidate = next(c for c in CANDIDATES if c.name == row.name)
            filename = f"{row.thread_index}-{candidate.view.value}.npz"
            matrix = (artifacts / filename).read_bytes()
            if hashlib.sha256(matrix).hexdigest() != report.artifact_sha256[filename]:
                raise click.ClickException("development matrix changed")
            with np.load(io.BytesIO(matrix), allow_pickle=False) as points:
                vectors = points["vectors"]
                owners = tuple(str(value) for value in points["turn_ids"])
            for prediction in row.predictions:
                ref = references[prediction.pair_id]
                label = labels.labels[ref.pair_id]
                pair = JudgedPair(
                    ref.pair_id, ref.a_turn_id, ref.b_turn_id, label.verdict
                )
                prefix, primary, stop = prefix_prediction(
                    vectors, owners, pair, candidate
                )
                observations.append(
                    Observation(
                        name=row.name,
                        thread_index=row.thread_index,
                        prompted_turns=row.prompted_turns,
                        pair_id=pair.id,
                        verdict=label.verdict,
                        cosine=float(
                            vectors[owners.index(pair.a)]
                            @ vectors[owners.index(pair.b)]
                        ),
                        prefix_turns=stop,
                        full=prediction.overlapping,
                        prefix=prefix,
                        full_primary=prediction.primary,
                        prefix_primary=primary,
                    )
                )
    frozen = tuple(observations)
    audit = Audit(
        development_sha256=hashlib.sha256(raw).hexdigest(),
        summaries=tuple(
            summarize(frozen, name, length) for name in DIAGNOSTICS for length in Length
        ),
        observations=frozen,
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as file:
        file.write(orjson.dumps(audit.model_dump(mode="json")))
    click.echo(orjson.dumps([s.model_dump() for s in audit.summaries]).decode())


if __name__ == "__main__":
    main()
