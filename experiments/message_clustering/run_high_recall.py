"""Bounded calibration for the user's simultaneous turn-clustering targets."""

import os
import time
from enum import StrEnum
from pathlib import Path

import click
import numpy as np
import optuna
import orjson
from threadpoolctl import threadpool_limits

from alternatives import Alternative, partition
from alternatives import Setting as AlternativeSetting
from clustering import (
    Configuration,
    Method,
    VectorMatrix,
    normalized_vectors,
    primary_partition,
)
from corpus import Record
from experiment_tracking import Summary, log_summary, tracked_run
from run_voyage_calibration import (
    Geometry,
    Outcome,
    Parameters,
    Prepared,
    Row,
    Split,
    View,
    geometry,
    metrics,
    outcome,
    rows,
)
from study_annotations import load_development
from study_scoring import JudgedPair, combine
from temporal_smoothing import Setting, lineages, smooth
from turn_evaluation import Counts
from turn_study_corpus import Cohort
from voyage_vectors import digest

RADII = (0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70)
MARGINS = (0.0, 0.025, 0.05, 0.10, 0.20, 0.30, 0.50, 1.0)
CASE = "voyage-code-4-2048-model_tokens"


class Family(StrEnum):
    BIRCH = "birch"
    AVERAGE = "average_link"
    COMPLETE = "complete_link"
    LEADER = "leader"


def alternative_geometry(
    vectors: VectorMatrix, owners: tuple[str, ...], cutoff: float, family: Family
) -> Geometry:
    if family == Family.BIRCH:
        return geometry(vectors, owners, cutoff)
    labels = np.array(
        partition(vectors, AlternativeSetting(Alternative.COMPLETE_LINK, cutoff))
        if family == Family.COMPLETE
        else primary_partition(vectors, Configuration(Method(family.value), cutoff))
    )
    groups = [
        g
        for g in sorted(set(labels))
        if g >= 0
        and len({o for o, label in zip(owners, labels, strict=True) if label == g}) >= 2
    ]
    centers = [vectors[labels == g].mean(axis=0) for g in groups]
    centers = [c / np.linalg.norm(c) for c in centers if np.linalg.norm(c) > 0]
    scores = (
        np.clip(vectors @ np.stack(centers).T, -1, 1)
        if centers
        else np.empty((len(owners), 0))
    )
    return Geometry(tuple(dict.fromkeys(owners)), owners, scores)


class Quality(Record):
    counts: Counts
    distant_related: int
    distant_missed: int

    @property
    def distant_recall(self) -> float:
        return (
            1 - self.distant_missed / self.distant_related
            if self.distant_related
            else 0.0
        )

    @property
    def feasible(self) -> bool:
        return (
            self.counts.precision > 0.82
            and self.counts.recall > 0.90
            and self.distant_recall > 0.80
        )

    @property
    def minimum_ratio(self) -> float:
        return min(
            self.counts.precision / 0.82,
            self.counts.recall / 0.90,
            self.distant_recall / 0.80,
        )

    @property
    def objective(self) -> float:
        # All infeasible settings rank below any feasible one.
        return (
            1 + self.counts.precision if self.feasible else min(self.minimum_ratio, 1.0)
        )


def quality(values: tuple[Row, ...]) -> Quality:
    return Quality(
        counts=combine(tuple(r.counts for r in values)),
        distant_related=sum(r.distant_related for r in values),
        distant_missed=sum(r.distant_missed for r in values),
    )


class Candidate(Record):
    number: int
    parameters: Parameters
    calibration: Quality
    code_weight: float = 1.0
    family: Family = Family.BIRCH


class Result(Record):
    selected: Candidate
    evaluation: Quality
    outcome: Outcome
    trials: tuple[Candidate, ...]
    elapsed_seconds: float
    contract_sha256: str


class Targets(Record):
    precision: float = 0.82
    recall: float = 0.90
    distant_recall: float = 0.80


class Contract(Record):
    case: str = CASE
    trials: int = 256
    seed: int = 20261007
    targets: Targets = Targets()
    confirmation: str = "sealed"
    status: str = "reused_development"
    split_sha256: str
    cohort_sha256: str
    reference_sha256: str
    protocol_sha256: str
    annotation_sha256: dict[str, str]
    matrix_sha256: dict[str, str]
    code_sha256: dict[str, str]
    code_weights: tuple[float, ...] = (1.0,)
    families: tuple[Family, ...] = (Family.BIRCH,)


def blend(code: VectorMatrix, general: VectorMatrix, weight: float) -> VectorMatrix:
    """Cosine in this space is a weighted sum of the two model cosines."""
    if len(code) != len(general) or not 0 <= weight <= 1:
        raise ValueError("ensemble requires aligned rows and a convex weight")
    return normalized_vectors(
        np.concatenate(
            (
                np.sqrt(weight) * normalized_vectors(code),
                np.sqrt(1 - weight) * normalized_vectors(general),
            ),
            axis=1,
        )
    )


def choose(candidates: tuple[Candidate, ...]) -> Candidate:
    feasible = tuple(c for c in candidates if c.calibration.feasible)
    if feasible:
        return max(
            feasible,
            key=lambda c: (
                c.calibration.counts.precision,
                c.calibration.counts.f1,
                -c.number,
            ),
        )
    return max(
        candidates,
        key=lambda c: (c.calibration.minimum_ratio, c.calibration.counts.f1, -c.number),
    )


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--matrices", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--ensemble", is_flag=True)
@click.option("--families", is_flag=True)
def main(
    root: Path,
    matrices: Path,
    output: Path,
    protocol: Path,
    ensemble: bool,
    families: bool,
) -> None:
    started = time.perf_counter()
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    if (output / "result.json").exists():
        raise ValueError("completed runs cannot be overwritten")
    reference_root = root / "voyage-calibration-20261006"
    split_raw = (reference_root / "split.json").read_bytes()
    split = Split.model_validate_json(split_raw)
    cohort_raw = (root / "turn-study-fleet-verified-20261005/cohort.json").read_bytes()
    cohort = Cohort.model_validate_json(cohort_raw)
    annotations = load_development(root / "turn-study-blind-20261005")
    reference_raw = (reference_root / "general-complete-selected.json").read_bytes()
    reference = Outcome.model_validate_json(reference_raw)
    paths = {
        i: matrices / f"{CASE}-{i}.npz" for i in split.calibration + split.evaluation
    }
    weights = (0.0, 0.25, 0.50, 0.75, 1.0) if ensemble else (1.0,)
    methods = (
        (Family.AVERAGE, Family.COMPLETE, Family.LEADER)
        if families
        else (Family.BIRCH,)
    )
    partition_cutoffs = (0.4, 0.5, 0.6, 0.7, 0.8) if families else RADII
    general_paths = (
        {i: matrices / f"voyage-4-1024-model_tokens-{i}.npz" for i in paths}
        if ensemble
        else {}
    )
    contract = Contract(
        split_sha256=digest(split_raw),
        cohort_sha256=digest(cohort_raw),
        reference_sha256=digest(reference_raw),
        protocol_sha256=digest(protocol.read_bytes()),
        annotation_sha256=annotations.file_sha256,
        matrix_sha256={
            p.name: digest(p.read_bytes())
            for p in (*paths.values(), *general_paths.values())
        },
        code_weights=weights,
        families=methods,
        trials=768 if families else 512 if ensemble else 256,
        code_sha256={
            n: digest(Path(__file__).with_name(n).read_bytes())
            for n in (
                "run_high_recall.py",
                "run_voyage_calibration.py",
                "clustering.py",
                "temporal_smoothing.py",
            )
        },
    )
    raw = orjson.dumps(contract.model_dump(mode="json"))
    (output / "contract.json").write_bytes(raw)
    training: dict[tuple[float, Family], list[Prepared]] = {
        (w, f): [] for w in weights for f in methods
    }
    testing: dict[tuple[float, Family], list[Prepared]] = {
        (w, f): [] for w in weights for f in methods
    }
    with threadpool_limits(limits=1):
        for entry in cohort.entries:
            index = entry.captured.index
            if index not in paths:
                continue
            with np.load(paths[index], allow_pickle=False) as stored:
                owners = tuple(str(o) for o in stored["pooled_owners"])
                chunk_owners = tuple(str(o) for o in stored["chunk_owners"])
                raw_pooled, raw_chunks = stored["pooled"], stored["chunks"]
                general_pooled, general_chunks = raw_pooled, raw_chunks
                if ensemble:
                    with np.load(general_paths[index], allow_pickle=False) as other:
                        if not np.array_equal(
                            other["pooled_owners"], stored["pooled_owners"]
                        ) or not np.array_equal(
                            other["chunk_owners"], stored["chunk_owners"]
                        ):
                            raise ValueError("ensemble source ownership differs")
                        general_pooled, general_chunks = (
                            other["pooled"],
                            other["chunks"],
                        )
                pairs = tuple(
                    JudgedPair(
                        r.pair_id,
                        r.a_turn_id,
                        r.b_turn_id,
                        annotations.labels[r.pair_id].verdict,
                    )
                    for r in annotations.manifest.references
                    if r.thread_index == index and r.pair_id in annotations.labels
                )
                for weight in weights:
                    pooled = (
                        blend(raw_pooled, general_pooled, weight)
                        if ensemble
                        else raw_pooled
                    )
                    chunks = (
                        blend(raw_chunks, general_chunks, weight)
                        if ensemble
                        else raw_chunks
                    )
                    pooled = smooth(
                        pooled,
                        owners,
                        lineages(entry.captured.thread, entry.captured.turns),
                        Setting(alpha=0.10, window=4),
                    )
                    for family in methods:
                        value = Prepared(
                            index,
                            owners,
                            pairs,
                            {
                                r: alternative_geometry(pooled, owners, r, family)
                                for r in partition_cutoffs
                            },
                            {
                                r: alternative_geometry(chunks, chunk_owners, r, family)
                                for r in partition_cutoffs
                            },
                        )
                        (training if index in split.calibration else testing)[
                            weight, family
                        ].append(value)
        storage = optuna.storages.InMemoryStorage()
        study = optuna.create_study(
            storage=storage,
            direction="maximize",
            sampler=optuna.samplers.TPESampler(seed=20261007),
        )
        if not families:
            study.enqueue_trial(
                {
                    "radius": 0.50,
                    "cutoff": 0.70,
                    "margin": 1.0,
                    "view": View.POOLED.value,
                    "code_weight": 1.0,
                }
            )
        candidates: list[Candidate] = []

        def objective(trial: optuna.Trial) -> float:
            weight = trial.suggest_categorical("code_weight", list(weights))
            family = Family(
                trial.suggest_categorical("family", [f.value for f in methods])
            )
            parameters = Parameters(
                radius=trial.suggest_categorical("radius", list(partition_cutoffs)),
                cutoff=trial.suggest_float("cutoff", 0.55, 0.85, step=0.0125),
                margin=trial.suggest_categorical("margin", list(MARGINS)),
                view=trial.suggest_categorical("view", [v.value for v in View]),
            )
            q = quality(rows(tuple(training[weight, family]), parameters))
            candidates.append(
                Candidate(
                    number=trial.number,
                    parameters=parameters,
                    calibration=q,
                    code_weight=weight,
                    family=family,
                )
            )
            for name, value in (
                ("precision", q.counts.precision),
                ("recall", q.counts.recall),
                ("distant_recall", q.distant_recall),
            ):
                trial.set_user_attr(name, value)
            return q.objective

        with tracked_run(
            root / "mlflow",
            "high-recall-clustering-v1",
            CASE,
            {"contract_sha256": digest(raw), "phase": "calibration"},
        ) as (client, run_id):
            study.optimize(objective, n_trials=contract.trials, n_jobs=1)
            chosen = choose(tuple(candidates))
            # Freeze selection before computing any development evaluation metric.
            (output / "selected.json").write_bytes(
                orjson.dumps(chosen.model_dump(mode="json"))
            )
            selected = outcome(
                CASE + "-high-recall",
                rows(
                    tuple(testing[chosen.code_weight, chosen.family]), chosen.parameters
                ),
                chosen.parameters,
                reference.rows,
            )
            result = Result(
                selected=chosen,
                evaluation=quality(selected.rows),
                outcome=selected,
                trials=tuple(candidates),
                elapsed_seconds=time.perf_counter() - started,
                contract_sha256=digest(raw),
            )
            (output / "result.json").write_bytes(
                orjson.dumps(result.model_dump(mode="json"))
            )
            log_summary(
                client,
                run_id,
                Summary(
                    baseline="voyage4-joint-v1",
                    treatment=CASE,
                    metrics=metrics(selected)
                    | {"target_met": float(result.evaluation.feasible)},
                    provenance={"contract_sha256": digest(raw)},
                ),
            )
        # Persist after selection, avoiding trial-time database writes.
        optuna.copy_study(
            from_study_name=study.study_name,
            from_storage=storage,
            to_storage=f"sqlite:///{output / 'optuna.sqlite'}",
            to_study_name="high-recall-clustering-v1",
        )
    click.echo(
        orjson.dumps(
            {
                "selected": chosen.parameters.model_dump(mode="json"),
                "evaluation": result.evaluation.model_dump(mode="json"),
                "target_met": result.evaluation.feasible,
            }
        ).decode()
    )


if __name__ == "__main__":
    main()
