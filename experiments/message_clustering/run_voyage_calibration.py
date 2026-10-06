"""Frozen-source encoder audit with bounded calibration and a DEV holdout."""

import os
import re
import time
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

import click
import numpy as np
import optuna
import orjson
from optuna.trial import TrialState
from threadpoolctl import threadpool_limits

from ablation_evidence import fit, fuse
from blind_turn_pairs import Verdict
from build_complete_answers import Artifact
from build_complete_conditioning import baseline
from build_input_followup import pool
from clustering import (
    Configuration,
    Method,
    VectorMatrix,
    normalized_vectors,
    primary_partition,
)
from corpus import Record
from experiment_tracking import Summary, log_summary, tracked_run
from followup_inputs import lean_prompt
from run_study_development import Interval, bootstrap
from study_annotations import load_development
from study_scoring import JudgedPair, combine, score
from temporal_smoothing import Setting, lineages, smooth
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Entry, Phase
from voyage_vectors import (
    Model,
    Receipt,
    build_cache,
    codec,
    digest,
    read_cache,
    structural_parts,
)

RADII = (0.40, 0.50, 0.60, 0.70)
CUTOFFS = (0.55, 0.65, 0.70, 0.75, 0.80, 0.85)
MARGINS = (0.00, 0.05, 0.10, 1.00)
EXCLUDED = frozenset((71, 73, 74, 110))
SECRETS = re.compile(
    r"\b(?:sk-(?:[a-z]+-)*[A-Za-z0-9_-]{20,512}|gh[pousr]_[A-Za-z0-9]{30,}"
    r"|github_pat_[A-Za-z0-9_]{30,}|AIza[A-Za-z0-9_-]{35}|(?:AKIA|ASIA)[A-Z0-9]{16}"
    r"|xox[baprs]-[A-Za-z0-9-]{20,255})\b"
    r"|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"
    r"|(?i:authorization[\"']?\s*[:=]\s*[\"']?(?:bearer|basic)\s+[^\s\"',;]+)"
    r"|(?i:(?:postgres(?:ql)?|redis|rediss|https?)://[^\s/@:]*:[^\s/@]+@)"
    r"|(?i:(?:password|passwd|api[_-]?key|client[_-]?secret|access[_-]?token)"
    r"[\"']?\s*[:=]\s*[\"'][^\"'\r\n]+[\"'])"
)


class View(StrEnum):
    POOLED = "pooled"
    CHUNK = "chunk"
    UNION = "union"


class Parameters(Record):
    radius: float = 0.60
    cutoff: float = 0.75
    margin: float = 0.05
    view: View = View.UNION


class Split(Record):
    calibration: tuple[int, ...]
    evaluation: tuple[int, ...]
    excluded: tuple[int, ...]
    source_sha256: dict[str, str]
    protocol_sha256: str
    code_sha256: dict[str, str]
    input_sha256: dict[str, tuple[str, ...]]
    confirmation: str = "sealed"


@dataclass(frozen=True)
class Inputs:
    entry: Entry
    artifact: Artifact
    complete: tuple[tuple[str, ...], ...]


@dataclass(frozen=True)
class Matrices:
    pooled: VectorMatrix
    chunks: VectorMatrix
    owners: tuple[str, ...]
    chunk_owners: tuple[str, ...]


@dataclass(frozen=True)
class Geometry:
    owners: tuple[str, ...]
    point_owners: tuple[str, ...]
    scores: VectorMatrix


@dataclass(frozen=True)
class Prepared:
    index: int
    owners: tuple[str, ...]
    pairs: tuple[JudgedPair, ...]
    pooled: dict[float, Geometry]
    chunks: dict[float, Geometry]


class Row(Record):
    thread_index: int
    long: bool
    counts: Counts
    predictions: dict[str, bool]
    distant_related: int
    distant_missed: int


class Outcome(Record):
    arm: str
    parameters: Parameters
    rows: tuple[Row, ...]
    overall: Counts
    long: Counts
    distant_recall: float
    interval: Interval
    long_interval: Interval | None


def split_indices(indices: tuple[int, ...]) -> tuple[tuple[int, ...], tuple[int, ...]]:
    ordered = sorted(
        indices, key=lambda i: digest(f"voyage-calibration-v1:{i}".encode())
    )
    stop = len(ordered) * 2 // 3
    return tuple(sorted(ordered[:stop])), tuple(sorted(ordered[stop:]))


def remaining_trials(study: optuna.Study, budget: int = 48) -> int:
    """Queued trials are not executions and must not consume the trial budget."""
    completed = sum(t.state == TrialState.COMPLETE for t in study.trials)
    return max(0, budget - completed)


def complete_inputs(entry: Entry) -> tuple[tuple[str, ...], ...]:
    thread = entry.captured.thread
    result = []
    for turn in entry.captured.turns:
        if not turn.prompts:
            continue
        segments = ["User:\n" + lean_prompt(thread, i).text for i in turn.prompts]
        segments.extend(
            "Assistant:\n" + thread.messages[i].projection
            for i in turn.answers
            if thread.messages[i].projection.strip()
        )
        result.append(structural_parts("\n\n".join(segments)))
    return tuple(result)


def geometry(vectors: VectorMatrix, owners: tuple[str, ...], radius: float) -> Geometry:
    if len(vectors) != len(owners):
        raise ValueError("ownership length differs from points")
    labels = np.array(
        primary_partition(vectors, Configuration(Method.BIRCH, 0.75, radius))
    )
    groups = [
        g
        for g in sorted(set(labels))
        if g >= 0
        and len({o for o, label in zip(owners, labels, strict=True) if label == g}) >= 2
    ]
    centres = [vectors[labels == g].mean(axis=0) for g in groups]
    centres = [c / np.linalg.norm(c) for c in centres if np.linalg.norm(c) > 0]
    scores = (
        np.clip(vectors @ np.stack(centres).T, -1, 1)
        if centres
        else np.empty((len(owners), 0))
    )
    return Geometry(tuple(dict.fromkeys(owners)), owners, scores)


def connections(value: Geometry, parameters: Parameters) -> dict[str, frozenset[int]]:
    groups: dict[str, set[int]] = {o: set() for o in value.owners}
    if value.scores.shape[1]:
        best = value.scores.max(axis=1, keepdims=True)
        mask = (value.scores >= parameters.cutoff) & (
            value.scores >= best - parameters.margin
        )
        for owner, membership in zip(value.point_owners, mask, strict=True):
            groups[owner].update(np.flatnonzero(membership).tolist())
    support = Counter(g for assigned in groups.values() for g in assigned)
    return {
        o: frozenset(g for g in assigned if support[g] >= 2)
        for o, assigned in groups.items()
    }


def predictions(value: Prepared, parameters: Parameters) -> dict[str, bool]:
    a = connections(value.pooled[parameters.radius], parameters)
    b = connections(value.chunks[parameters.radius], parameters)
    return {
        p.id: (
            (parameters.view != View.CHUNK and bool(a[p.a] & a[p.b]))
            or (parameters.view != View.POOLED and bool(b[p.a] & b[p.b]))
        )
        for p in value.pairs
    }


def rows(values: tuple[Prepared, ...], parameters: Parameters) -> tuple[Row, ...]:
    result = []
    for value in values:
        pred = predictions(value, parameters)
        long = len(value.owners) >= 8
        related = [
            p
            for p in value.pairs
            if long
            and p.verdict == Verdict.RELATED
            and abs(value.owners.index(p.a) - value.owners.index(p.b)) >= 4
        ]
        result.append(
            Row(
                thread_index=value.index,
                long=long,
                counts=score(value.pairs, pred),
                predictions=pred,
                distant_related=len(related),
                distant_missed=sum(not pred[p.id] for p in related),
            )
        )
    return tuple(result)


def outcome(
    arm: str,
    selected: tuple[Row, ...],
    parameters: Parameters,
    control: tuple[Row, ...],
) -> Outcome:
    if tuple(r.thread_index for r in selected) != tuple(
        r.thread_index for r in control
    ):
        raise ValueError("bootstrap thread pairing differs")
    long = tuple(r.counts for r in selected if r.long)
    original_long = tuple(r.counts for r in control if r.long)
    total = sum(r.distant_related for r in selected)
    return Outcome(
        arm=arm,
        parameters=parameters,
        rows=selected,
        overall=combine(tuple(r.counts for r in selected)),
        long=combine(long),
        distant_recall=1 - sum(r.distant_missed for r in selected) / total
        if total
        else 0,
        interval=bootstrap(
            tuple(r.counts for r in selected), tuple(r.counts for r in control)
        ),
        long_interval=bootstrap(long, original_long) if long else None,
    )


def matrix(
    inputs: Inputs, cache: dict[str, VectorMatrix], quantized: bool, complete: bool
) -> Matrices:
    turns = tuple(t for t in inputs.entry.captured.turns if t.prompts)
    owners = tuple(t.id for t in turns)
    if complete:
        chunks = normalized_vectors(
            np.concatenate(
                [
                    cache[digest(text.encode())]
                    for parts in inputs.complete
                    for text in parts
                ]
            )
        )
        chunk_owners = tuple(
            owner
            for owner, parts in zip(owners, inputs.complete, strict=True)
            for _ in parts
        )
        pooled = normalized_vectors(
            np.stack(
                [
                    chunks[[i for i, o in enumerate(chunk_owners) if o == owner]].mean(
                        axis=0
                    )
                    for owner in owners
                ]
            )
        )
    else:
        points = np.concatenate(
            [
                cache[digest(item.content.text.encode())]
                for item in inputs.artifact.pooled_inputs + inputs.artifact.chunk_inputs
            ]
        )
        points = codec(points) if quantized else normalized_vectors(points)
        pooled = pool(
            turns,
            inputs.artifact.pooled_inputs,
            points[: len(inputs.artifact.pooled_inputs)],
        )
        chunks = points[len(inputs.artifact.pooled_inputs) :]
        chunk_owners = tuple(item.turn_id for item in inputs.artifact.chunk_inputs)
    return Matrices(pooled, chunks, owners, chunk_owners)


def metrics(value: Outcome) -> dict[str, float]:
    result = {
        f"{prefix}.{key}": float(getattr(counts, key))
        for prefix, counts in (("overall", value.overall), ("long", value.long))
        for key in ("tp", "fp", "fn", "tn", "precision", "recall", "f1")
    }
    result.update(
        {
            "distant.recall": value.distant_recall,
            "overall.gain_low": value.interval.gain_low,
            "overall.gain_high": value.interval.gain_high,
        }
    )
    return result


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--key-file", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--prepare-only", is_flag=True)
def main(
    root: Path, output: Path, protocol: Path, key_file: Path, prepare_only: bool
) -> None:
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    ref = root / "complete-answer-baseline-20261006"
    frozen = baseline(ref)
    cohort = Cohort.model_validate_json(
        (root / "turn-study-fleet-verified-20261005/cohort.json").read_bytes()
    )
    eligible = []
    excluded = set(EXCLUDED)
    sources = {}
    for entry in cohort.entries:
        if entry.phase != Phase.DEVELOPMENT or entry.captured.index in EXCLUDED:
            continue
        index = entry.captured.index
        artifact = Artifact.model_validate_json(
            (ref / "artifacts" / f"{index}-all.json").read_bytes()
        )
        complete = complete_inputs(entry)
        texts = tuple(
            item.content.text for item in artifact.pooled_inputs + artifact.chunk_inputs
        ) + tuple(text for parts in complete for text in parts)
        if any(SECRETS.search(text) for text in texts):
            excluded.add(index)
            continue
        for suffix in (".json", ".npz"):
            name = f"{index}-all{suffix}"
            actual = digest((ref / "artifacts" / name).read_bytes())
            if actual != frozen.artifact_sha256[name]:
                raise ValueError("frozen source changed")
            sources[name] = actual
        eligible.append(Inputs(entry, artifact, complete))
    calibration, evaluation = split_indices(
        tuple(i.entry.captured.index for i in eligible)
    )
    contract = Split(
        calibration=calibration,
        evaluation=evaluation,
        excluded=tuple(sorted(excluded)),
        source_sha256=sources,
        protocol_sha256=digest(protocol.read_bytes()),
        code_sha256={
            name: digest(Path(__file__).with_name(name).read_bytes())
            for name in (
                "run_voyage_calibration.py",
                "voyage_vectors.py",
                "clustering.py",
                "build_input_followup.py",
                "temporal_smoothing.py",
            )
        },
        input_sha256={
            str(i.entry.captured.index): tuple(
                digest(text.encode()) for parts in i.complete for text in parts
            )
            for i in eligible
        },
    )
    raw = orjson.dumps(contract.model_dump(mode="json"))
    path = output / "split.json"
    if path.exists() and path.read_bytes() != raw:
        raise ValueError("registered split/source contract changed")
    path.write_bytes(raw)
    targeted = sum(
        item.target is not None
        for i in eligible
        for item in i.artifact.pooled_inputs + i.artifact.chunk_inputs
    )
    click.echo(
        f"eligible={len(eligible)} calibration={len(calibration)} "
        f"evaluation={len(evaluation)} excluded={len(excluded)} "
        f"targeted_inputs={targeted}"
    )
    if prepare_only:
        return
    texts = tuple(
        dict.fromkeys(
            [
                item.content.text
                for i in eligible
                for item in i.artifact.pooled_inputs + i.artifact.chunk_inputs
            ]
            + [text for i in eligible for parts in i.complete for text in parts]
        )
    )
    for model in Model:
        build_cache(output, texts, model, key_file)
    labels = load_development(root / "turn-study-blind-20261005")
    controls: tuple[Row, ...] = ()
    selected_controls: tuple[Row, ...] = ()
    arms = (
        "minilm",
        "code-float",
        "code-int8",
        "code-complete",
        "general-float",
        "general-int8",
        "general-complete",
    )
    audit = {
        "targeted_inputs": targeted,
        "old_code_codec_max_abs": 0.0,
        "old_code_codec_mean_cosine": 0.0,
        "checked_histories": 0,
    }
    old_cosines = []
    with threadpool_limits(limits=1):
        for arm in arms:
            started = time.perf_counter()
            cache = (
                read_cache(
                    output, Model.CODE if arm.startswith("code") else Model.GENERAL
                )
                if arm != "minilm"
                else {}
            )
            values = []
            for i in eligible:
                index = i.entry.captured.index
                with np.load(
                    ref / "artifacts" / f"{index}-all.npz", allow_pickle=False
                ) as saved:
                    original = Matrices(
                        saved["pooled"],
                        saved["chunks"],
                        tuple(saved["pooled_owners"].tolist()),
                        tuple(saved["chunk_owners"].tolist()),
                    )
                selected = (
                    original
                    if arm == "minilm"
                    else matrix(
                        i, cache, arm.endswith("int8"), arm.endswith("complete")
                    )
                )
                if selected.owners != original.owners or set(
                    selected.chunk_owners
                ) != set(selected.owners):
                    raise ValueError("turn ownership drift")
                for points in (selected.pooled, selected.chunks):
                    np.testing.assert_allclose(
                        np.linalg.norm(points, axis=1), 1, atol=1e-6
                    )
                if arm == "code-int8":
                    with np.load(
                        root / "voyage-code4-20261006" / f"{index}.npz",
                        allow_pickle=False,
                    ) as old:
                        for name, points in (
                            ("pooled", selected.pooled),
                            ("chunks", selected.chunks),
                        ):
                            if points.shape != old[name].shape:
                                raise ValueError("old API control shape mismatch")
                            audit["old_code_codec_max_abs"] = max(
                                audit["old_code_codec_max_abs"],
                                float(np.abs(points - old[name]).max()),
                            )
                            old_cosines.extend(
                                np.sum(points * old[name], axis=1).tolist()
                            )
                    audit["checked_histories"] += 1
                np.savez_compressed(
                    output / f"{arm}-{index}.npz",
                    pooled=selected.pooled,
                    chunks=selected.chunks,
                    pooled_owners=np.array(selected.owners),
                    chunk_owners=np.array(selected.chunk_owners),
                )
                a = smooth(
                    selected.pooled,
                    selected.owners,
                    lineages(i.entry.captured.thread, i.entry.captured.turns),
                    Setting(alpha=0.10, window=4),
                )
                pairs = tuple(
                    JudgedPair(
                        r.pair_id,
                        r.a_turn_id,
                        r.b_turn_id,
                        labels.labels[r.pair_id].verdict,
                    )
                    for r in labels.manifest.references
                    if r.thread_index == index and r.pair_id in labels.labels
                )
                value = Prepared(
                    index,
                    selected.owners,
                    pairs,
                    {radius: geometry(a, selected.owners, radius) for radius in RADII},
                    {
                        radius: geometry(selected.chunks, selected.chunk_owners, radius)
                        for radius in RADII
                    },
                )
                if arm == "minilm":
                    expected = fuse(
                        fit(a, selected.owners),
                        fit(selected.chunks, selected.chunk_owners),
                        (0.0,) * len(selected.chunk_owners),
                        False,
                    )
                    actual = predictions(value, Parameters())
                    assert actual == {
                        p.id: bool(expected[p.a] & expected[p.b]) for p in pairs
                    }
                    assert actual == next(
                        r.predictions for r in frozen.rows if r.thread_index == index
                    )
                values.append(value)
            training = tuple(v for v in values if v.index in calibration)
            testing = tuple(v for v in values if v.index in evaluation)
            if arm == "minilm":
                controls = rows(testing, Parameters())
            study = optuna.create_study(
                study_name=arm,
                storage=f"sqlite:///{output / 'optuna.sqlite'}",
                direction="maximize",
                sampler=optuna.samplers.RandomSampler(seed=20261006),
                load_if_exists=True,
            )
            if not study.trials:
                study.enqueue_trial(Parameters().model_dump(mode="json"))

            def objective(
                trial: optuna.Trial,
                training: tuple[Prepared, ...] = training,
                arm: str = arm,
            ) -> float:
                parameters = Parameters(
                    radius=trial.suggest_categorical("radius", list(RADII)),
                    cutoff=trial.suggest_categorical("cutoff", list(CUTOFFS)),
                    margin=trial.suggest_categorical("margin", list(MARGINS)),
                    view=trial.suggest_categorical("view", [v.value for v in View]),
                )
                counts = combine(tuple(r.counts for r in rows(training, parameters)))
                with tracked_run(
                    root / "mlflow",
                    "voyage-calibration-v1",
                    f"{arm}-trial-{trial.number}",
                    {
                        "arm": arm,
                        "phase": "calibration",
                        "contract_sha256": digest(raw),
                        **{
                            k: str(v)
                            for k, v in parameters.model_dump(mode="json").items()
                        },
                    },
                ) as (client, run_id):
                    trial.set_user_attr("mlflow_run_id", run_id)
                    log_summary(
                        client,
                        run_id,
                        Summary(
                            treatment=arm,
                            metrics={
                                "calibration.f1": counts.f1,
                                "calibration.precision": counts.precision,
                                "calibration.recall": counts.recall,
                            },
                            provenance={"contract_sha256": digest(raw)},
                        ),
                    )
                return counts.f1

            study.optimize(objective, n_trials=remaining_trials(study), n_jobs=1)
            chosen = Parameters.model_validate(study.best_trial.params)
            selected_rows = rows(testing, chosen)
            if arm == "minilm":
                selected_controls = selected_rows
            outcomes = [outcome(arm + "-selected", selected_rows, chosen, controls)]
            outcomes.extend(
                outcome(
                    arm + "-fixed-" + view.value,
                    rows(testing, Parameters(view=view)),
                    Parameters(view=view),
                    controls,
                )
                for view in View
            )
            for result in outcomes:
                (output / f"{result.arm}.json").write_bytes(
                    orjson.dumps(result.model_dump(mode="json"))
                )
                with tracked_run(
                    root / "mlflow",
                    "voyage-calibration-v1",
                    result.arm,
                    {
                        "phase": "evaluation",
                        "arm": arm,
                        "contract_sha256": digest(raw),
                        "selection": "calibration-only",
                    },
                ) as (client, run_id):
                    log_summary(
                        client,
                        run_id,
                        Summary(
                            treatment=result.arm,
                            metrics=metrics(result),
                            provenance={"contract_sha256": digest(raw)},
                        ),
                    )
            comparison = outcome(arm, selected_rows, chosen, selected_controls)
            (output / f"{arm}-vs-calibrated-minilm.json").write_bytes(
                orjson.dumps(comparison.model_dump(mode="json"))
            )
            click.echo(
                f"{arm} selected={chosen.model_dump(mode='json')} "
                f"eval F1={outcomes[0].overall.f1:.4f} "
                f"long={outcomes[0].long.f1:.4f} "
                f"seconds={time.perf_counter() - started:.1f}"
            )
    audit["old_code_codec_mean_cosine"] = float(np.mean(old_cosines))
    (output / "audit.json").write_bytes(orjson.dumps(audit))
    receipts = tuple(
        Receipt.model_validate_json(line)
        for line in (output / "receipts.jsonl").read_bytes().splitlines()
    )
    click.echo("total API cost: " + str(sum(r.cost for r in receipts)))


if __name__ == "__main__":
    main()
