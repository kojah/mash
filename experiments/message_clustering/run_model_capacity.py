"""Model-specific joint-turn capacities, with frozen-reference replay and ablations."""

import os
import sqlite3
import time
import tomllib
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

import click
import numpy as np
import optuna
import orjson
from pydantic import ConfigDict
from threadpoolctl import threadpool_limits

from clustering import normalized_vectors
from corpus import Record
from experiment_tracking import Summary, log_summary, tracked_run
from followup_inputs import lean_prompt
from model_capacity import Capacity, TokenBudget
from run_voyage_calibration import (
    CUTOFFS,
    MARGINS,
    RADII,
    SECRETS,
    Matrices,
    Outcome,
    Parameters,
    Prepared,
    Split,
    View,
    geometry,
    metrics,
    outcome,
    remaining_trials,
    rows,
)
from study_annotations import load_development
from study_scoring import JudgedPair, combine
from temporal_smoothing import Setting, lineages, smooth
from turn_study_corpus import Cohort, Entry
from voyage_vectors import (
    Model,
    Receipt,
    build_cache,
    cache_space,
    digest,
    read_cache,
    structural_parts,
)


class BudgetKind(StrEnum):
    BYTES = "legacy_bytes"
    TOKENS = "model_tokens"


class BaselineAlgorithm(Record):
    model_config = ConfigDict(extra="ignore", frozen=True)
    birch_radius: float
    membership_cosine_cutoff: float
    relative_best_margin: float
    smoothing_alpha: float
    smoothing_window: int


class BaselineProvenance(Record):
    model_config = ConfigDict(extra="ignore", frozen=True)
    private_run_directory: Path
    selected_report: str
    selected_report_sha256: str
    split_contract_sha256: str


class Baseline(Record):
    model_config = ConfigDict(extra="ignore", frozen=True)
    id: str
    algorithm: BaselineAlgorithm
    provenance: BaselineProvenance


class Case(Record):
    capacity: Capacity
    budget: BudgetKind

    @property
    def name(self) -> str:
        return (
            f"{self.capacity.model.value.split('/')[-1]}-"
            f"{self.capacity.dimensions}-{self.budget}"
        )


@dataclass(frozen=True)
class Source:
    entry: Entry
    owners: tuple[str, ...]
    texts: tuple[str, ...]


class Contract(Record):
    baseline_id: str
    baseline_manifest_sha256: str
    reference_report_sha256: str
    cohort_sha256: str
    split_sha256: str
    protocol_sha256: str
    code_sha256: dict[str, str]
    annotation_sha256: dict[str, str]
    cases: tuple[Case, ...]
    input_sha256: dict[str, tuple[str, ...]]
    confirmation: str = "sealed"
    evaluation_status: str = "previously_consumed_development"


def source(entry: Entry) -> Source:
    texts, owners = [], []
    for turn in entry.captured.turns:
        if not turn.prompts:
            continue
        thread = entry.captured.thread
        parts = ["User:\n" + lean_prompt(thread, i).text for i in turn.prompts]
        parts.extend(
            "Assistant:\n" + thread.messages[i].projection
            for i in turn.answers
            if thread.messages[i].projection.strip()
        )
        texts.append("\n\n".join(parts))
        owners.append(turn.id)
    return Source(entry, tuple(owners), tuple(texts))


def pieces(
    value: Source, case: Case, budget: TokenBudget
) -> tuple[tuple[str, ...], ...]:
    return tuple(
        structural_parts(text)
        if case.budget == BudgetKind.BYTES
        else budget.parts(text)
        for text in value.texts
    )


def matrices(
    value: Source, parts: tuple[tuple[str, ...], ...], cache: dict[str, np.ndarray]
) -> Matrices:
    chunks = normalized_vectors(
        np.concatenate(
            [cache[digest(text.encode())] for group in parts for text in group]
        )
    )
    owners = tuple(
        owner for owner, group in zip(value.owners, parts, strict=True) for _ in group
    )
    pooled = normalized_vectors(
        np.stack(
            [
                chunks[[i for i, owner in enumerate(owners) if owner == turn]].mean(
                    axis=0
                )
                for turn in value.owners
            ]
        )
    )
    return Matrices(pooled, chunks, value.owners, owners)


def import_reference_cache(root: Path, output: Path, cases: tuple[Case, ...]) -> None:
    """Only copy exact model/task/dimension-compatible raw API outputs."""
    original = root / "voyage-calibration-20261006/raw-vectors.sqlite"
    with sqlite3.connect(original.as_uri() + "?mode=ro", uri=True) as old:
        with sqlite3.connect(output / "raw-vectors.sqlite") as db:
            db.execute(
                "CREATE TABLE IF NOT EXISTS vectors "
                "(model TEXT, hash TEXT, vector BLOB, PRIMARY KEY(model,hash))"
            )
            for case in cases:
                if case.capacity.dimensions != 1024:
                    continue
                db.executemany(
                    "INSERT OR IGNORE INTO vectors VALUES (?,?,?)",
                    (
                        (
                            cache_space(case.capacity.model, case.capacity),
                            identity,
                            blob,
                        )
                        for identity, blob in old.execute(
                            "SELECT hash,vector FROM vectors WHERE model=?",
                            (case.capacity.model,),
                        )
                    ),
                )


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--baseline", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--key-file", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--dimensions", type=int, multiple=True, default=(1024, 2048))
@click.option("--input-tokens", type=int, default=31992)
@click.option("--prepare-only", is_flag=True)
def main(
    root: Path,
    output: Path,
    baseline: Path,
    protocol: Path,
    key_file: Path,
    dimensions: tuple[int, ...],
    input_tokens: int,
    prepare_only: bool,
) -> None:
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    manifest = Baseline.model_validate(tomllib.loads(baseline.read_text()))
    reference_root = manifest.provenance.private_run_directory
    raw_reference = (reference_root / manifest.provenance.selected_report).read_bytes()
    if digest(raw_reference) != manifest.provenance.selected_report_sha256:
        raise ValueError("frozen reference report changed")
    reference = Outcome.model_validate_json(raw_reference)
    split_raw = (reference_root / "split.json").read_bytes()
    if digest(split_raw) != manifest.provenance.split_contract_sha256:
        raise ValueError("frozen development split changed")
    split = Split.model_validate_json(split_raw)
    cohort_raw = (root / "turn-study-fleet-verified-20261005/cohort.json").read_bytes()
    cohort = Cohort.model_validate_json(cohort_raw)
    sources = tuple(
        source(e)
        for e in cohort.entries
        if e.captured.index in split.calibration + split.evaluation
    )
    if any(SECRETS.search(text) for s in sources for text in s.texts):
        raise ValueError("joint input credential screen failed before upload")
    cases = []
    budgets = {}
    for model in Model:
        path = output / "tokenizers" / f"{model.value.split('/')[-1]}-tokenizer.json"
        for dimension in dimensions:
            capacity = Capacity(
                model=model,
                dimensions=dimension,
                input_tokens=input_tokens,
                tokenizer_sha256=digest(path.read_bytes()),
            )
            budget = TokenBudget(path, capacity)
            for kind in BudgetKind:
                case = Case(capacity=capacity, budget=kind)
                cases.append(case)
                budgets[case.name] = budget
    built = {
        case.name: {
            s.entry.captured.index: pieces(s, case, budgets[case.name]) for s in sources
        }
        for case in cases
    }
    labels = load_development(root / "turn-study-blind-20261005")
    contract = Contract(
        baseline_id=manifest.id,
        baseline_manifest_sha256=digest(baseline.read_bytes()),
        reference_report_sha256=digest(raw_reference),
        cohort_sha256=digest(cohort_raw),
        split_sha256=digest(split_raw),
        protocol_sha256=digest(protocol.read_bytes()),
        annotation_sha256=labels.file_sha256,
        cases=tuple(cases),
        code_sha256={
            name: digest(Path(__file__).with_name(name).read_bytes())
            for name in (
                "run_model_capacity.py",
                "model_capacity.py",
                "voyage_vectors.py",
                "run_voyage_calibration.py",
                "temporal_smoothing.py",
                "clustering.py",
            )
        },
        input_sha256={
            case.name: tuple(
                digest(text.encode())
                for s in sources
                for parts in built[case.name][s.entry.captured.index]
                for text in parts
            )
            for case in cases
        },
    )
    raw = orjson.dumps(contract.model_dump(mode="json"))
    path = output / "contract.json"
    if path.exists() and path.read_bytes() != raw:
        raise ValueError("resume contract changed")
    path.write_bytes(raw)
    click.echo(
        f"prepared {len(cases)} cases / {len(sources)} histories; confirmation sealed"
    )
    if prepare_only:
        return
    import_reference_cache(root, output, tuple(cases))
    for case in cases:
        texts = tuple(
            dict.fromkeys(
                text
                for s in sources
                for group in built[case.name][s.entry.captured.index]
                for text in group
            )
        )
        build_cache(output, texts, case.capacity.model, key_file, budgets[case.name])
    baseline_parameters = Parameters(
        radius=manifest.algorithm.birch_radius,
        cutoff=manifest.algorithm.membership_cosine_cutoff,
        margin=manifest.algorithm.relative_best_margin,
        view=View.UNION,
    )
    with threadpool_limits(limits=1):
        for case in cases:
            started = time.perf_counter()
            cache = read_cache(output, case.capacity.model, case.capacity)
            values = []
            for s in sources:
                index = s.entry.captured.index
                points = matrices(s, built[case.name][index], cache)
                np.savez_compressed(
                    output / f"{case.name}-{index}.npz",
                    pooled=points.pooled,
                    chunks=points.chunks,
                    pooled_owners=np.array(points.owners),
                    chunk_owners=np.array(points.chunk_owners),
                )
                a = smooth(
                    points.pooled,
                    points.owners,
                    lineages(s.entry.captured.thread, s.entry.captured.turns),
                    Setting(
                        alpha=manifest.algorithm.smoothing_alpha,
                        window=manifest.algorithm.smoothing_window,
                    ),
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
                values.append(
                    Prepared(
                        index,
                        points.owners,
                        pairs,
                        {
                            radius: geometry(a, points.owners, radius)
                            for radius in RADII
                        },
                        {
                            radius: geometry(points.chunks, points.chunk_owners, radius)
                            for radius in RADII
                        },
                    )
                )
            training = tuple(v for v in values if v.index in split.calibration)
            testing = tuple(v for v in values if v.index in split.evaluation)
            fixed = rows(testing, baseline_parameters)
            if (
                case.capacity.model == Model.GENERAL
                and case.capacity.dimensions == 1024
                and case.budget == BudgetKind.BYTES
            ):
                assert tuple(r.predictions for r in fixed) == tuple(
                    r.predictions for r in reference.rows
                )
            study = optuna.create_study(
                study_name=case.name,
                storage=f"sqlite:///{output / 'optuna.sqlite'}",
                direction="maximize",
                sampler=optuna.samplers.RandomSampler(seed=20261006),
                load_if_exists=True,
            )
            if not study.trials:
                study.enqueue_trial(baseline_parameters.model_dump(mode="json"))

            def objective(
                trial: optuna.Trial,
                training: tuple[Prepared, ...] = training,
                case: Case = case,
            ) -> float:
                p = Parameters(
                    radius=trial.suggest_categorical("radius", list(RADII)),
                    cutoff=trial.suggest_categorical("cutoff", list(CUTOFFS)),
                    margin=trial.suggest_categorical("margin", list(MARGINS)),
                    view=trial.suggest_categorical("view", [v.value for v in View]),
                )
                counts = combine(tuple(r.counts for r in rows(training, p)))
                with tracked_run(
                    root / "mlflow",
                    "voyage-model-capacity-v1",
                    f"{case.name}-{trial.number}",
                    {
                        "case": case.name,
                        "phase": "calibration",
                        "contract_sha256": digest(raw),
                    },
                ) as (client, run_id):
                    trial.set_user_attr("mlflow_run_id", run_id)
                    log_summary(
                        client,
                        run_id,
                        Summary(
                            baseline=manifest.id,
                            treatment=case.name,
                            metrics={"calibration.f1": counts.f1},
                            provenance={"contract_sha256": digest(raw)},
                        ),
                    )
                return counts.f1

            study.optimize(objective, n_trials=remaining_trials(study), n_jobs=1)
            chosen = Parameters.model_validate(study.best_trial.params)
            for label, p in (("fixed", baseline_parameters), ("selected", chosen)):
                result = outcome(
                    case.name + "-" + label, rows(testing, p), p, reference.rows
                )
                (output / f"{result.arm}.json").write_bytes(
                    orjson.dumps(result.model_dump(mode="json"))
                )
                with tracked_run(
                    root / "mlflow",
                    "voyage-model-capacity-v1",
                    result.arm,
                    {
                        "phase": "development_evaluation",
                        "case": case.name,
                        "contract_sha256": digest(raw),
                    },
                ) as (client, run_id):
                    log_summary(
                        client,
                        run_id,
                        Summary(
                            baseline=manifest.id,
                            treatment=result.arm,
                            metrics=metrics(result),
                            provenance={"contract_sha256": digest(raw)},
                        ),
                    )
                click.echo(
                    f"{result.arm}: P={result.overall.precision:.4f} "
                    f"R={result.overall.recall:.4f} F1={result.overall.f1:.4f} "
                    f"distant={result.distant_recall:.4f}"
                )
            click.echo(f"{case.name} complete in {time.perf_counter() - started:.1f}s")
    receipts = tuple(
        Receipt.model_validate_json(line)
        for line in (output / "receipts.jsonl").read_bytes().splitlines()
    )
    click.echo(f"new API charges ${sum(r.cost for r in receipts):.6f}")


if __name__ == "__main__":
    main()
