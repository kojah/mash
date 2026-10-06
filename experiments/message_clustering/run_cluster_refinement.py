"""Split weak overlapping centroid groups while preserving cluster memberships."""

import os
import time
from dataclasses import dataclass
from pathlib import Path

import click
import numpy as np
import optuna
import orjson
from threadpoolctl import threadpool_limits

from alternatives import Alternative, partition
from alternatives import Setting as LinkSetting
from blind_turn_pairs import Verdict
from clustering import VectorMatrix
from corpus import Record
from experiment_tracking import Summary, log_summary, tracked_run
from run_high_recall import (
    Family,
    Quality,
    alternative_geometry,
    blend,
    quality,
)
from run_high_recall import (
    Result as SweepResult,
)
from run_voyage_calibration import (
    Geometry,
    Outcome,
    Parameters,
    Row,
    Split,
    View,
    connections,
    metrics,
    outcome,
)
from study_annotations import load_development
from study_scoring import JudgedPair, score
from temporal_smoothing import Setting, lineages, smooth
from turn_study_corpus import Cohort
from voyage_vectors import digest

FLOORS = (0.0, 0.05, 0.15, 0.25, 0.35, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70)


class Base(Record):
    name: str
    parameters: Parameters
    family: Family
    weight: float


@dataclass(frozen=True)
class Data:
    index: int
    owners: tuple[str, ...]
    vectors: VectorMatrix
    pairs: tuple[JudgedPair, ...]
    pooled: Geometry
    chunks: Geometry


def refine(
    memberships: dict[str, frozenset[int]],
    vectors: VectorMatrix,
    owners: tuple[str, ...],
    cutoff: float,
) -> dict[str, frozenset[int]]:
    if cutoff == 0:
        return memberships
    if len(owners) != len(vectors) or len(set(owners)) != len(owners):
        raise ValueError("refinement requires one vector per distinct turn")
    refined: dict[str, set[int]] = {o: set() for o in owners}
    identity = 0
    for group in sorted(set().union(*memberships.values())):
        indices = [i for i, o in enumerate(owners) if group in memberships[o]]
        labels = partition(
            vectors[indices], LinkSetting(Alternative.COMPLETE_LINK, cutoff)
        )
        for label in sorted(set(labels)):
            members = [
                owners[i]
                for i, assigned in zip(indices, labels, strict=True)
                if assigned == label
            ]
            if len(members) >= 2:
                for owner in members:
                    refined[owner].add(identity)
                identity += 1
    return {o: frozenset(tags) for o, tags in refined.items()}


def evaluate(values: tuple[Data, ...], base: Base, cutoff: float) -> tuple[Row, ...]:
    result = []
    for value in values:
        a = refine(
            connections(value.pooled, base.parameters),
            value.vectors,
            value.owners,
            cutoff,
        )
        b = refine(
            connections(value.chunks, base.parameters),
            value.vectors,
            value.owners,
            cutoff,
        )
        predictions = {
            p.id: (
                (base.parameters.view != View.CHUNK and bool(a[p.a] & a[p.b]))
                or (base.parameters.view != View.POOLED and bool(b[p.a] & b[p.b]))
            )
            for p in value.pairs
        }
        # Reuse the exact registered definition of long/distant from the other sweeps.
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
                counts=score(value.pairs, predictions),
                predictions=predictions,
                distant_related=len(related),
                distant_missed=sum(not predictions[p.id] for p in related),
            )
        )
    return tuple(result)


class Candidate(Record):
    number: int
    base: int
    cutoff: float
    calibration: Quality


class Result(Record):
    selected: Candidate
    base: Base
    evaluation: Quality
    outcome: Outcome
    trials: tuple[Candidate, ...]
    elapsed_seconds: float
    contract_sha256: str


class Contract(Record):
    bases: tuple[Base, ...]
    floors: tuple[float, ...] = FLOORS
    matrix_sha256: dict[str, str]
    protocol_sha256: str
    code_sha256: dict[str, str]
    annotation_sha256: dict[str, str]
    input_sha256: dict[str, str]
    confirmation: str = "sealed"
    status: str = "reused_development"


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
def main(root: Path, output: Path, protocol: Path) -> None:
    started = time.perf_counter()
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    if (output / "result.json").exists():
        raise ValueError("completed runs cannot be overwritten")
    reference_root = root / "voyage-calibration-20261006"
    split = Split.model_validate_json((reference_root / "split.json").read_bytes())
    reference = Outcome.model_validate_json(
        (reference_root / "general-complete-selected.json").read_bytes()
    )
    original = Outcome.model_validate_json(
        (
            root
            / "voyage-model-capacity-20261006"
            / "voyage-code-4-2048-model_tokens-selected.json"
        ).read_bytes()
    )
    bases = [
        Base(
            name="code", parameters=original.parameters, family=Family.BIRCH, weight=1.0
        )
    ]
    for name in ("high-recall-ensemble-20261006", "high-recall-families-20261006"):
        selected = SweepResult.model_validate_json(
            (root / name / "result.json").read_bytes()
        ).selected
        bases.append(
            Base(
                name=name,
                parameters=selected.parameters,
                family=selected.family,
                weight=selected.code_weight,
            )
        )
    annotations = load_development(root / "turn-study-blind-20261005")
    cohort = Cohort.model_validate_json(
        (root / "turn-study-fleet-verified-20261005/cohort.json").read_bytes()
    )
    paths = [
        root / "voyage-model-capacity-20261006" / f"{case}-{i}.npz"
        for i in split.calibration + split.evaluation
        for case in ("voyage-code-4-2048-model_tokens", "voyage-4-1024-model_tokens")
    ]
    hashes = {p.name: digest(p.read_bytes()) for p in paths}
    contract = Contract(
        bases=tuple(bases),
        matrix_sha256=hashes,
        protocol_sha256=digest(protocol.read_bytes()),
        code_sha256={
            n: digest(Path(__file__).with_name(n).read_bytes())
            for n in (
                "run_cluster_refinement.py",
                "run_high_recall.py",
                "alternatives.py",
                "clustering.py",
                "run_voyage_calibration.py",
                "temporal_smoothing.py",
            )
        },
        annotation_sha256=annotations.file_sha256,
        input_sha256={
            str(p): digest(p.read_bytes())
            for p in (
                reference_root / "split.json",
                reference_root / "general-complete-selected.json",
                root / "turn-study-fleet-verified-20261005/cohort.json",
            )
        },
    )
    raw = orjson.dumps(contract.model_dump(mode="json"))
    (output / "contract.json").write_bytes(raw)
    training: dict[int, list[Data]] = {i: [] for i in range(len(bases))}
    testing: dict[int, list[Data]] = {i: [] for i in range(len(bases))}
    with threadpool_limits(limits=1):
        for entry in cohort.entries:
            index = entry.captured.index
            if index not in split.calibration + split.evaluation:
                continue
            matrix_root = root / "voyage-model-capacity-20261006"
            with (
                np.load(
                    matrix_root / f"voyage-code-4-2048-model_tokens-{index}.npz"
                ) as code,
                np.load(
                    matrix_root / f"voyage-4-1024-model_tokens-{index}.npz"
                ) as general,
            ):
                owners = tuple(str(o) for o in code["pooled_owners"])
                assert np.array_equal(code["pooled_owners"], code["chunk_owners"])
                assert np.array_equal(general["pooled_owners"], general["chunk_owners"])
                assert np.array_equal(code["pooled_owners"], general["pooled_owners"])
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
                for i, base in enumerate(bases):
                    vectors = blend(code["pooled"], general["pooled"], base.weight)
                    pooled = smooth(
                        vectors,
                        owners,
                        lineages(entry.captured.thread, entry.captured.turns),
                        Setting(alpha=0.10, window=4),
                    )
                    value = Data(
                        index,
                        owners,
                        vectors,
                        pairs,
                        alternative_geometry(
                            pooled, owners, base.parameters.radius, base.family
                        ),
                        alternative_geometry(
                            vectors, owners, base.parameters.radius, base.family
                        ),
                    )
                    (training if index in split.calibration else testing)[i].append(
                        value
                    )
        storage = optuna.storages.InMemoryStorage()
        study = optuna.create_study(
            direction="maximize",
            storage=storage,
            sampler=optuna.samplers.GridSampler(
                {"base": list(range(len(bases))), "cutoff": list(FLOORS)}, seed=20261008
            ),
        )
        candidates: list[Candidate] = []

        def objective(trial: optuna.Trial) -> float:
            index = trial.suggest_categorical("base", list(range(len(bases))))
            cutoff = trial.suggest_categorical("cutoff", list(FLOORS))
            q = quality(evaluate(tuple(training[index]), bases[index], cutoff))
            candidates.append(
                Candidate(number=trial.number, base=index, cutoff=cutoff, calibration=q)
            )
            return q.objective

        with tracked_run(
            root / "mlflow",
            "high-recall-refinement-v1",
            "complete-link-refinement",
            {"contract_sha256": digest(raw)},
        ) as (client, run_id):
            study.optimize(objective, n_trials=len(bases) * len(FLOORS))
            chosen = max(
                candidates,
                key=lambda c: (
                    c.calibration.objective,
                    c.calibration.counts.f1,
                    -c.number,
                ),
            )
            (output / "selected.json").write_bytes(
                orjson.dumps(chosen.model_dump(mode="json"))
            )
            selected = outcome(
                "complete-link-refinement",
                evaluate(
                    tuple(testing[chosen.base]), bases[chosen.base], chosen.cutoff
                ),
                bases[chosen.base].parameters,
                reference.rows,
            )
            result = Result(
                selected=chosen,
                base=bases[chosen.base],
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
                    treatment="complete-link-refinement",
                    metrics=metrics(selected)
                    | {"target_met": float(result.evaluation.feasible)},
                    provenance={"contract_sha256": digest(raw)},
                ),
            )
        optuna.copy_study(
            from_study_name=study.study_name,
            from_storage=storage,
            to_storage=f"sqlite:///{output / 'optuna.sqlite'}",
            to_study_name="refinement",
        )
    click.echo(orjson.dumps(result.evaluation.model_dump(mode="json")).decode())


if __name__ == "__main__":
    main()
