"""Two registered Optuna trials with local MLflow tracking and fixed scoring."""

import os
import time
from enum import StrEnum
from pathlib import Path

import click
import optuna
import orjson
from optuna.trial import TrialState

from build_complete_answers import Contract
from build_complete_conditioning import BASELINE_SHA, AuditCatalogue, Details, baseline
from build_token_weighting import digest
from experiment_tracking import Summary, log_summary, metrics, tracked_run
from followup_inputs import AnswerScope
from run_complete_answers import Result, evaluate


class Arm(StrEnum):
    NONE = "none"
    FIRST80 = "first80"


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--reference", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--tracking", type=click.Path(path_type=Path), required=True)
def main(
    parent: Path, reference: Path, artifacts: Path, output: Path, tracking: Path
) -> None:
    os.umask(0o077)
    frozen = baseline(reference)
    raw = (artifacts / "contract.json").read_bytes()
    contract = Contract.model_validate_json(raw)
    catalogue_raw = (artifacts / "audit-catalogue.json").read_bytes()
    catalogue = AuditCatalogue.model_validate_json(catalogue_raw)
    if catalogue.contract_sha256 != digest(raw) or len(catalogue.files) != 160:
        raise ValueError("conditioning catalogue invalid")
    for filename, expected in catalogue.files.items():
        if digest((artifacts / filename).read_bytes()) != expected:
            raise ValueError("conditioning audit changed")
    for filename, expected in contract.code_sha256.items():
        if digest(Path(__file__).with_name(filename).read_bytes()) != expected:
            raise ValueError("candidate builder source changed")
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    study = optuna.create_study(
        study_name="complete-answer-conditioning-v1",
        storage=f"sqlite:///{(output / 'optuna.sqlite').resolve()}",
        direction="maximize",
        load_if_exists=True,
        sampler=optuna.samplers.RandomSampler(seed=0),
    )
    pinned = {
        "baseline_sha256": BASELINE_SHA,
        "contract_sha256": digest(raw),
        "audit_catalogue_sha256": digest(catalogue_raw),
        "tracking_directory": str(tracking.resolve()),
        "scorer_sha256": digest(Path(__file__).read_bytes()),
        "tracking_code_sha256": digest(
            Path(__file__).with_name("experiment_tracking.py").read_bytes()
        ),
    }
    if study.user_attrs.get("contract", pinned) != pinned or (
        study.trials and "contract" not in study.user_attrs
    ):
        raise ValueError("study contract changed")
    if any(trial.state == TrialState.RUNNING for trial in study.trials):
        raise ValueError("live/interrupted trial needs explicit recovery")
    study.set_user_attr("contract", pinned)
    scheduled = {
        trial.params.get("conditioning", trial.user_attrs.get("conditioning"))
        for trial in study.trials
        if trial.state in (TrialState.COMPLETE, TrialState.WAITING)
    }
    for arm in Arm:
        if arm.value not in scheduled:
            study.enqueue_trial(
                {"conditioning": arm.value}, user_attrs={"conditioning": arm.value}
            )

    def objective(trial: optuna.Trial) -> float:
        arm = Arm(trial.suggest_categorical("conditioning", [arm.value for arm in Arm]))
        parameters = {
            **pinned,
            "baseline": "complete-answer-v1",
            "conditioning": arm.value,
            "protocol_sha256": contract.protocol_sha256,
            "encoder_generation": contract.encoder_generation,
            "answer_scope": "all",
            "inference": "singleton",
            "smoothing_alpha": "0.10",
            "smoothing_window": "4",
            "birch_radius": "0.60",
            "membership_cutoff": "0.75",
            "fusion": "namespaced_union",
            "prompt_budget": "80" if arm == Arm.FIRST80 else "0",
            "pooling": "answer_only" if arm == Arm.FIRST80 else "original",
        }
        with tracked_run(
            tracking, study.study_name, f"{arm}-trial-{trial.number}", parameters
        ) as (client, run_id):
            trial.set_user_attr("mlflow_run_id", run_id)
            started = time.perf_counter()
            control = (
                None
                if arm == Arm.NONE
                else Result.model_validate_json((output / "none.json").read_bytes())
            )
            result = evaluate(
                parent,
                reference / "artifacts" if arm == Arm.NONE else artifacts,
                AnswerScope.ALL,
                control,
            )
            if arm == Arm.NONE and tuple(
                (row.counts, row.predictions) for row in result.rows
            ) != tuple((row.counts, row.predictions) for row in frozen.rows):
                raise ValueError("baseline predictions differ")
            (output / f"{arm}.json").write_bytes(
                orjson.dumps(result.model_dump(mode="json"))
            )
            values = metrics(result)
            values["score_seconds"] = time.perf_counter() - started
            if arm == Arm.FIRST80:
                details = tuple(
                    Details.model_validate_json(path.read_bytes())
                    for path in artifacts.glob("*-conditioning.json")
                )
                if len(details) != 160 or any(
                    row.contract_sha256 != digest(raw) for row in details
                ):
                    raise ValueError("conditioning audit incomplete")
                values.update(
                    {
                        "build_seconds": sum(row.seconds for row in details),
                        "new_vectors": float(sum(row.new_vectors for row in details)),
                        "original_chunks": float(
                            sum(row.original_chunks for row in details)
                        ),
                        "answer_chunks": float(
                            sum(row.answer_chunks for row in details)
                        ),
                        "conditioned_chunks": float(
                            sum(row.conditioned_chunks for row in details)
                        ),
                        "truncated_prompts": float(
                            sum(row.truncated_prompts for row in details)
                        ),
                    }
                )
            log_summary(
                client,
                run_id,
                Summary(
                    baseline="complete-answer-v1",
                    treatment=arm.value,
                    metrics=values,
                    provenance=pinned,
                ),
            )
            client.log_artifact(
                run_id,
                str(
                    Path(__file__).parents[2]
                    / "docs/complete-answer-conditioning-protocol.md"
                ),
            )
            click.echo(
                f"{arm}: F1={result.overall.f1:.6f} "
                f"long_R={result.long.recall:.6f} mlflow={run_id}"
            )
            return result.overall.f1

    study.optimize(
        objective,
        n_trials=sum(trial.state == TrialState.WAITING for trial in study.trials),
        n_jobs=1,
    )
    for arm in Arm:
        result = Result.model_validate_json((output / f"{arm}.json").read_bytes())
        expected = frozen.contract_sha256 if arm == Arm.NONE else digest(raw)
        if result.contract_sha256 != expected or len(result.rows) != 160:
            raise ValueError("completed report changed")


if __name__ == "__main__":
    main()
