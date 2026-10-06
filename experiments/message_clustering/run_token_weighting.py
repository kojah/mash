"""Score four declared pooling trials; do not optimize outside the frozen grid."""

import io
import os
import time
from collections.abc import Callable
from pathlib import Path

import click
import numpy as np
import optuna
import orjson
from optuna.trial import TrialState
from threadpoolctl import threadpool_limits

from ablation_evidence import fit, fuse
from build_token_weighting import Artifact, Contract, digest
from corpus import Record
from freeze_link_review import SOURCE_SHA
from run_study_development import Interval, bootstrap
from run_turn_ablation import AblationReport
from study_annotations import load_development
from study_scoring import JudgedPair, absolute_gate, combine, score
from temporal_smoothing import Setting, lineages, smooth
from token_weighting import Policy
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase


class Row(Record):
    thread_index: int
    long: bool
    counts: Counts
    predictions: dict[str, bool]
    seconds: float


class Result(Record):
    policy: Policy
    rows: tuple[Row, ...]
    overall: Counts
    long: Counts
    interval: Interval
    long_interval: Interval
    historical_interval: Interval
    eligible: bool
    changed_predictions: int
    artifacts_sha256: dict[str, str]
    annotation_sha256: dict[str, str]
    contract_sha256: str
    scoring_code_sha256: dict[str, str]
    confirmation: str = "sealed"


def score_policy(
    parent: Path, artifacts: Path, policy: Policy, control: Result | None
) -> Result:
    contract_raw = (artifacts / "contract.json").read_bytes()
    contract = Contract.model_validate_json(contract_raw)
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    report_raw = (
        parent / "turn-three-factor-ablation-20261006/development.json"
    ).read_bytes()
    if digest(cohort_raw) != contract.cohort_sha256 or digest(report_raw) != SOURCE_SHA:
        raise ValueError("frozen parent changed")
    cohort = Cohort.model_validate_json(cohort_raw)
    report = AblationReport.model_validate_json(report_raw)
    labels = load_development(parent / "turn-study-blind-20261005")
    if (
        labels.file_sha256 != report.report.annotation_sha256
        or labels.manifest_sha256 != report.report.annotation_manifest_sha256
    ):
        raise ValueError("annotations changed")
    historical = {r.thread_index: r for r in report.report.rows if r.name == "c0f0g0"}
    rows = []
    hashes = {}
    with threadpool_limits(limits=1):
        for entry in cohort.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            captured = entry.captured
            path = artifacts / f"{captured.index}.json"
            raw = path.read_bytes()
            metadata = Artifact.model_validate_json(raw)
            vector_raw = path.with_suffix(".npz").read_bytes()
            if (
                metadata.thread_index != captured.index
                or metadata.contract_sha256 != digest(contract_raw)
                or metadata.matrix_sha256 != digest(vector_raw)
                or metadata.source_sha256
                != digest(orjson.dumps(captured.model_dump(mode="json")))
            ):
                raise ValueError("artifact provenance changed")
            hashes[path.name] = digest(raw)
            hashes[path.with_suffix(".npz").name] = digest(vector_raw)
            with np.load(io.BytesIO(vector_raw), allow_pickle=False) as archive:
                a = archive[f"pooled_{policy}"]
                b = archive[f"chunks_{policy}"]
                a_owners = tuple(archive["pooled_owners"].tolist())
                b_owners = tuple(archive["chunk_owners"].tolist())
            if a_owners != tuple(t.id for t in captured.turns if t.prompts):
                raise ValueError("pooled ownership changed")
            for vectors, owners in ((a, a_owners), (b, b_owners)):
                if (
                    vectors.shape != (len(owners), 384)
                    or not np.isfinite(vectors).all()
                    or not np.allclose(np.linalg.norm(vectors, axis=1), 1, atol=1e-9)
                ):
                    raise ValueError("invalid point matrix")
            started = time.perf_counter()
            members = fuse(
                fit(
                    smooth(
                        a,
                        a_owners,
                        lineages(captured.thread, captured.turns),
                        Setting(alpha=0.10, window=4),
                    ),
                    a_owners,
                ),
                fit(b, b_owners),
                (0.0,) * len(b_owners),
                False,
            )
            pairs = tuple(
                JudgedPair(
                    r.pair_id,
                    r.a_turn_id,
                    r.b_turn_id,
                    labels.labels[r.pair_id].verdict,
                )
                for r in labels.manifest.references
                if r.thread_index == captured.index and r.pair_id in labels.labels
            )
            predictions = {p.id: bool(members[p.a] & members[p.b]) for p in pairs}
            rows.append(
                Row(
                    thread_index=captured.index,
                    long=len(a_owners) >= 8,
                    counts=score(pairs, predictions),
                    predictions=predictions,
                    seconds=time.perf_counter() - started,
                )
            )
    overall = combine(tuple(r.counts for r in rows))
    long_rows = tuple(r for r in rows if r.long)
    long = combine(tuple(r.counts for r in long_rows))
    controls = control.rows if control is not None else tuple(rows)
    if tuple(r.thread_index for r in rows) != tuple(r.thread_index for r in controls):
        raise ValueError("control identities differ")
    interval = bootstrap(
        tuple(r.counts for r in rows), tuple(r.counts for r in controls)
    )
    long_interval = bootstrap(
        tuple(r.counts for r in long_rows), tuple(r.counts for r in controls if r.long)
    )
    return Result(
        policy=policy,
        rows=tuple(rows),
        overall=overall,
        long=long,
        interval=interval,
        long_interval=long_interval,
        historical_interval=bootstrap(
            tuple(r.counts for r in rows),
            tuple(historical[r.thread_index].counts for r in rows),
        ),
        eligible=(
            policy != Policy.UNIFORM
            and absolute_gate(overall)
            and absolute_gate(long)
            and interval.gain_low > 0
            and long_interval.gain_low > 0
        ),
        changed_predictions=sum(
            prediction != base.predictions[key]
            for row, base in zip(rows, controls, strict=True)
            for key, prediction in row.predictions.items()
        ),
        artifacts_sha256=hashes,
        annotation_sha256=labels.file_sha256,
        contract_sha256=digest(contract_raw),
        scoring_code_sha256={
            name: digest(Path(__file__).with_name(name).read_bytes())
            for name in (
                "run_token_weighting.py",
                "ablation_evidence.py",
                "temporal_smoothing.py",
                "clustering.py",
                "study_scoring.py",
            )
        },
    )


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(parent: Path, artifacts: Path, output: Path) -> None:
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    contract = Contract.model_validate_json((artifacts / "contract.json").read_bytes())

    def evaluate(policy: Policy) -> float:
        control = (
            None
            if policy == Policy.UNIFORM
            else Result.model_validate_json((output / "uniform.json").read_bytes())
        )
        result = score_policy(parent, artifacts, policy, control)
        (output / f"{policy}.json").write_bytes(
            orjson.dumps(result.model_dump(mode="json"))
        )
        click.echo(f"{policy}: F1={result.overall.f1:.6f} eligible={result.eligible}")
        return result.overall.f1

    run_ledger(output / "optuna.sqlite", contract, evaluate)
    for policy in Policy:
        result = Result.model_validate_json((output / f"{policy}.json").read_bytes())
        if (
            result.policy != policy
            or result.contract_sha256
            != digest((artifacts / "contract.json").read_bytes())
            or len(result.rows) != 160
        ):
            raise ValueError("completed trial lacks its pinned report")


def run_ledger(
    database: Path, contract: Contract, evaluate: Callable[[Policy], float]
) -> optuna.Study:
    """Resume only a pinned four-policy ledger; refuse live or unowned trials."""
    if contract.policies != tuple(Policy):
        raise ValueError("study must contain exactly the four declared policies")
    study = optuna.create_study(
        study_name="token-downweighting",
        storage=f"sqlite:///{database.resolve()}",
        direction="maximize",
        load_if_exists=True,
        sampler=optuna.samplers.RandomSampler(seed=0),
    )
    metadata = contract.model_dump(mode="json")
    if study.trials and "contract" not in study.user_attrs:
        raise ValueError("existing trials lack a pinned contract")
    if study.user_attrs.get("contract", metadata) != metadata:
        raise ValueError("study contract changed")
    if any(t.state == TrialState.RUNNING for t in study.trials):
        raise ValueError("live/interrupted trial requires explicit recovery")
    study.set_user_attr("contract", metadata)
    scheduled = {
        t.params.get("policy", t.user_attrs.get("policy"))
        for t in study.trials
        if t.state in (TrialState.COMPLETE, TrialState.WAITING)
    }
    for policy in Policy:
        if policy.value not in scheduled:
            study.enqueue_trial(
                {"policy": policy.value}, user_attrs={"policy": policy.value}
            )

    def objective(trial: optuna.Trial) -> float:
        policy = Policy(trial.suggest_categorical("policy", [p.value for p in Policy]))
        return evaluate(policy)

    study.optimize(
        objective,
        n_trials=sum(t.state == TrialState.WAITING for t in study.trials),
        n_jobs=1,
    )
    return study


if __name__ == "__main__":
    main()
