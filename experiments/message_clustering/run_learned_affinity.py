"""Grouped calibration of a small, supervised, thread-local affinity model."""

import os
import time
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

import click
import joblib
import numpy as np
import optuna
import orjson
import sklearn
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression
from sklearn.model_selection import GroupKFold
from sklearn.preprocessing import StandardScaler
from threadpoolctl import threadpool_limits

from blind_turn_pairs import Verdict
from clustering import VectorMatrix
from corpus import Record
from experiment_tracking import Summary, log_summary, tracked_run
from run_high_recall import Quality, quality
from run_voyage_calibration import Outcome, Parameters, Row, Split, metrics, outcome
from study_annotations import load_development
from study_scoring import JudgedPair, score
from turn_affinity_features import NAMES, Features, LinearScore, build, edge_cover
from turn_study_corpus import Cohort
from voyage_vectors import digest


class Family(StrEnum):
    LOGISTIC = "logistic"
    HISTOGRAM = "histogram"


class FeatureSet(StrEnum):
    DENSE = "dense"
    HYBRID = "hybrid"


class Settings(Record):
    family: Family
    features: FeatureSet
    cutoff: float
    regularization: float
    leaves: int = 3
    iterations: int = 50
    min_leaf: int = 20


@dataclass(frozen=True)
class Fitted:
    scaler: StandardScaler
    model: LogisticRegression | HistGradientBoostingClassifier
    features: FeatureSet

    def predict(self, matrix: VectorMatrix) -> np.ndarray:
        selected = matrix[:, :4] if self.features == FeatureSet.DENSE else matrix
        return np.asarray(
            self.model.predict_proba(self.scaler.transform(selected))[:, 1]
        )


def fit(matrix: VectorMatrix, labels: np.ndarray, settings: Settings) -> Fitted:
    selected = matrix[:, :4] if settings.features == FeatureSet.DENSE else matrix
    scaler = StandardScaler().fit(selected)
    values = scaler.transform(selected)
    model = (
        LogisticRegression(
            C=settings.regularization, max_iter=2000, random_state=20261009
        )
        if settings.family == Family.LOGISTIC
        else HistGradientBoostingClassifier(
            max_leaf_nodes=settings.leaves,
            max_iter=settings.iterations,
            min_samples_leaf=settings.min_leaf,
            l2_regularization=settings.regularization,
            early_stopping=False,
            random_state=20261009,
        )
    )
    model.fit(values, labels)
    return Fitted(scaler, model, settings.features)


def portable(model: Fitted) -> LinearScore:
    if not isinstance(model.model, LogisticRegression):
        raise ValueError("portable linear export requires a logistic model")
    scale = np.asarray(model.scaler.scale_, dtype=np.float64)
    mean = np.asarray(model.scaler.mean_, dtype=np.float64)
    weights = model.model.coef_[0] / scale
    bias = model.model.intercept_[0] - np.sum(model.model.coef_[0] * mean / scale)
    names = NAMES[:4] if model.features == FeatureSet.DENSE else NAMES
    return LinearScore(
        features=names, weights=tuple(weights.tolist()), bias=float(bias)
    )


@dataclass(frozen=True)
class Data:
    features: Features
    pairs: tuple[JudgedPair, ...]
    reference_indices: tuple[int, ...]


def row(value: Data, predictions: dict[str, bool]) -> Row:
    positions = {o: i for i, o in enumerate(value.features.owners)}
    long = len(positions) >= 8
    related = [
        p
        for p in value.pairs
        if long
        and p.verdict == Verdict.RELATED
        and abs(positions[p.a] - positions[p.b]) >= 4
    ]
    return Row(
        thread_index=value.features.index,
        long=long,
        counts=score(value.pairs, predictions),
        predictions=predictions,
        distant_related=len(related),
        distant_missed=sum(not predictions[p.id] for p in related),
    )


class Candidate(Record):
    number: int
    settings: Settings
    calibration: Quality


class Result(Record):
    selected: Candidate
    evaluation: Quality
    outcome: Outcome
    candidates: tuple[Candidate, ...]
    groups: int
    pair_groups: int
    group_sizes: tuple[int, ...]
    elapsed_seconds: float
    contract_sha256: str
    model_sha256: str


class Contract(Record):
    protocol_sha256: str
    source_sha256: dict[str, str]
    matrix_sha256: dict[str, str]
    input_sha256: dict[str, str]
    annotation_sha256: dict[str, str]
    features: tuple[str, ...] = NAMES
    sklearn: str = sklearn.__version__
    folds: tuple[tuple[tuple[int, ...], tuple[int, ...]], ...]
    trials: int = 64
    confirmation: str = "sealed"
    status: str = "supervised_reused_development"


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
def main(root: Path, output: Path, protocol: Path) -> None:
    started = time.perf_counter()
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    if (output / "result.json").exists():
        raise ValueError("completed studies cannot be overwritten")
    ref_root = root / "voyage-calibration-20261006"
    split_path, reference_path = (
        ref_root / "split.json",
        ref_root / "general-complete-selected.json",
    )
    split = Split.model_validate_json(split_path.read_bytes())
    reference = Outcome.model_validate_json(reference_path.read_bytes())
    cohort_path = root / "turn-study-fleet-verified-20261005/cohort.json"
    cohort = Cohort.model_validate_json(cohort_path.read_bytes())
    annotations = load_development(root / "turn-study-blind-20261005")
    data: dict[int, Data] = {}
    hashes = {}
    with threadpool_limits(limits=1):
        for entry in cohort.entries:
            index = entry.captured.index
            if index not in split.calibration + split.evaluation:
                continue
            paths = tuple(
                root / "voyage-model-capacity-20261006" / f"{case}-{index}.npz"
                for case in (
                    "voyage-code-4-2048-model_tokens",
                    "voyage-4-1024-model_tokens",
                )
            )
            hashes.update({p.name: digest(p.read_bytes()) for p in paths})
            with (
                np.load(paths[0], allow_pickle=False) as code,
                np.load(paths[1], allow_pickle=False) as general,
            ):
                assert np.array_equal(code["pooled_owners"], general["pooled_owners"])
                owners = tuple(str(o) for o in code["pooled_owners"])
                features = build(entry, owners, code["pooled"], general["pooled"])
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
            positions = {o: i for i, o in enumerate(owners)}
            indices = {p: i for i, p in enumerate(features.pairs)}
            data[index] = Data(
                features,
                pairs,
                tuple(
                    indices[
                        min(positions[p.a], positions[p.b]),
                        max(positions[p.a], positions[p.b]),
                    ]
                    for p in pairs
                ),
            )
            np.savez_compressed(
                output / f"features-{index}.npz",
                values=features.values,
                pairs=np.array(features.pairs),
                owners=np.array(owners),
            )
        training = tuple(data[i] for i in sorted(split.calibration))
        samples = tuple(
            (d, p, j)
            for d in training
            for p, j in zip(d.pairs, d.reference_indices, strict=True)
            if p.verdict != Verdict.AMBIGUOUS
        )
        x = np.stack([d.features.values[j] for d, p, j in samples])
        y = np.array([p.verdict == Verdict.RELATED for d, p, j in samples], dtype=int)
        groups = np.array([d.features.index for d, p, j in samples])
        folds = tuple(GroupKFold(n_splits=5).split(x, y, groups))
        contract = Contract(
            protocol_sha256=digest(protocol.read_bytes()),
            source_sha256={
                n: digest(Path(__file__).with_name(n).read_bytes())
                for n in (
                    "run_learned_affinity.py",
                    "turn_affinity_features.py",
                    "study_scoring.py",
                    "temporal_smoothing.py",
                    "followup_inputs.py",
                )
            },
            matrix_sha256=hashes,
            input_sha256={
                str(p): digest(p.read_bytes())
                for p in (split_path, reference_path, cohort_path)
            },
            annotation_sha256=annotations.file_sha256,
            folds=tuple(
                (
                    tuple(sorted(set(groups[a].tolist()))),
                    tuple(sorted(set(groups[b].tolist()))),
                )
                for a, b in folds
            ),
        )
        assert all(not set(a) & set(b) for a, b in contract.folds)
        raw = orjson.dumps(contract.model_dump(mode="json"))
        (output / "contract.json").write_bytes(raw)
        storage = optuna.storages.InMemoryStorage()
        study = optuna.create_study(
            direction="maximize",
            storage=storage,
            sampler=optuna.samplers.TPESampler(seed=20261009),
        )
        candidates: list[Candidate] = []

        def objective(trial: optuna.Trial) -> float:
            family = Family(
                trial.suggest_categorical("family", [f.value for f in Family])
            )
            settings = Settings(
                family=family,
                features=FeatureSet(
                    trial.suggest_categorical("features", [f.value for f in FeatureSet])
                ),
                cutoff=trial.suggest_float("cutoff", 0.15, 0.85, step=0.025),
                regularization=trial.suggest_categorical(
                    "regularization", [0.1, 1.0, 10.0]
                ),
                leaves=trial.suggest_categorical("leaves", [3, 7])
                if family == Family.HISTOGRAM
                else 3,
                iterations=trial.suggest_categorical("iterations", [50, 100])
                if family == Family.HISTOGRAM
                else 50,
                min_leaf=trial.suggest_categorical("min_leaf", [10, 20, 40])
                if family == Family.HISTOGRAM
                else 20,
            )
            probabilities = np.zeros(len(y))
            for a, b in folds:
                probabilities[b] = fit(x[a], y[a], settings).predict(x[b])
            predictions = {
                p.id: bool(prob >= settings.cutoff)
                for (_, p, _), prob in zip(samples, probabilities, strict=True)
            }
            selected = tuple(
                row(d, {p.id: predictions.get(p.id, False) for p in d.pairs})
                for d in training
            )
            q = quality(selected)
            candidates.append(
                Candidate(number=trial.number, settings=settings, calibration=q)
            )
            for name, val in (
                ("precision", q.counts.precision),
                ("recall", q.counts.recall),
                ("distant_recall", q.distant_recall),
            ):
                trial.set_user_attr(name, val)
            return q.objective

        with tracked_run(
            root / "mlflow",
            "learned-turn-affinity-v1",
            "grouped-affinity",
            {"contract_sha256": digest(raw), "supervised": "true"},
        ) as (client, run_id):
            study.optimize(objective, n_trials=64, n_jobs=1)
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
            model = fit(x, y, chosen.settings)
            # Native sklearn objects have importable module identities even under -m.
            joblib.dump((model.scaler, model.model), output / "model.joblib")
            if isinstance(model.model, LogisticRegression):
                linear = portable(model)
                np.testing.assert_allclose(
                    linear.predict(x), model.predict(x), atol=1e-12
                )
                (output / "linear-model.json").write_bytes(
                    orjson.dumps(linear.model_dump(mode="json"))
                )
            model_hash = digest((output / "model.joblib").read_bytes())
            selected_rows, sizes = [], []
            for index in reference.rows:
                d = data[index.thread_index]
                probabilities = model.predict(d.features.values)
                n = len(d.features.owners)
                matrix = np.zeros((n, n))
                for (i, j), p in zip(d.features.pairs, probabilities, strict=True):
                    matrix[i, j] = matrix[j, i] = p
                clusters = edge_cover(matrix, chosen.settings.cutoff)
                sizes.extend(len(g) for g in clusters)
                membership: list[set[int]] = [set() for _ in range(n)]
                for tag, group in enumerate(clusters):
                    for owner in group:
                        membership[owner].add(tag)
                positions = {o: i for i, o in enumerate(d.features.owners)}
                pred = {
                    p.id: bool(membership[positions[p.a]] & membership[positions[p.b]])
                    for p in d.pairs
                }
                direct = {
                    p.id: bool(probabilities[j] >= chosen.settings.cutoff)
                    for p, j in zip(d.pairs, d.reference_indices, strict=True)
                }
                assert pred == direct
                selected_rows.append(row(d, pred))
                (output / f"clusters-{d.features.index}.json").write_bytes(
                    orjson.dumps(clusters)
                )
            # Actual learned settings are stored in the selected candidate.
            selected = outcome(
                "learned-turn-affinity",
                tuple(selected_rows),
                Parameters(),
                reference.rows,
            )
            result = Result(
                selected=chosen,
                evaluation=quality(selected.rows),
                outcome=selected,
                candidates=tuple(candidates),
                groups=len(sizes),
                pair_groups=sizes.count(2),
                group_sizes=tuple(sizes),
                elapsed_seconds=time.perf_counter() - started,
                contract_sha256=digest(raw),
                model_sha256=model_hash,
            )
            (output / "result.json").write_bytes(
                orjson.dumps(result.model_dump(mode="json"))
            )
            log_summary(
                client,
                run_id,
                Summary(
                    baseline="voyage4-joint-v1",
                    treatment="learned-turn-affinity",
                    metrics=metrics(selected)
                    | {"target_met": float(result.evaluation.feasible)},
                    provenance={
                        "contract_sha256": digest(raw),
                        "model_sha256": model_hash,
                    },
                ),
            )
        optuna.copy_study(
            from_study_name=study.study_name,
            from_storage=storage,
            to_storage=f"sqlite:///{output / 'optuna.sqlite'}",
            to_study_name="learned-affinity",
        )
    click.echo(orjson.dumps(result.evaluation.model_dump(mode="json")).decode())


if __name__ == "__main__":
    main()
