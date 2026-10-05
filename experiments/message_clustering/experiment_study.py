"""Bounded Optuna ledger for the preregistered context/boilerplate comparisons."""

from collections.abc import Callable
from enum import StrEnum
from pathlib import Path

import optuna
from optuna.trial import TrialState

from corpus import Record


class Context(StrEnum):
    NONE = "none"
    FIRST = "first80"
    SELECTED = "selected80"


class Setting(Record):
    context: Context
    boilerplate: bool


class Contract(Record):
    protocol_sha256: str
    cohort_sha256: str
    encoder_generation: str
    settings: tuple[Setting, ...]


def run(
    database: Path,
    name: str,
    contract: Contract,
    evaluate: Callable[[Setting], float],
) -> optuna.Study:
    """Run only declared settings; resume completed trials, reject changed inputs.

    The objective reports development F1. Acceptance gates and held-out judgment
    remain in the experiment report; Optuna's best trial is not a promotion.
    Failed trials are retried on an explicit rerun, not silently substituted.
    Single-process execution preserves fixed order and bounded inference memory.
    """
    if not contract.settings or len(set(contract.settings)) != len(contract.settings):
        raise ValueError("declare distinct nonempty experiment settings")
    study = optuna.create_study(
        study_name=name,
        storage=f"sqlite:///{database.resolve()}",
        direction="maximize",
        sampler=optuna.samplers.RandomSampler(seed=0),
        load_if_exists=True,
    )
    metadata = contract.model_dump(mode="json")
    prior = study.user_attrs.get("contract")
    if prior is not None and prior != metadata:
        raise ValueError("study contract changed; do not reuse this ledger")
    if prior is None:
        if study.trials:
            raise ValueError("existing trials lack a pinned study contract")
        study.set_user_attr("contract", metadata)
    scheduled = set()
    for trial in study.trials:
        if trial.state == TrialState.RUNNING:
            raise ValueError("live/interrupted trial requires explicit recovery")
        if trial.state in (TrialState.COMPLETE, TrialState.WAITING):
            scheduled.add(Setting.model_validate(trial.user_attrs["setting"]))
    for setting in contract.settings:
        if setting not in scheduled:
            study.enqueue_trial(
                setting.model_dump(mode="json"),
                user_attrs={"setting": setting.model_dump(mode="json")},
            )
    remaining = sum(t.state == TrialState.WAITING for t in study.trials)

    def objective(trial: optuna.Trial) -> float:
        setting = Setting(
            context=Context(
                trial.suggest_categorical("context", [c.value for c in Context])
            ),
            boilerplate=trial.suggest_categorical("boilerplate", [False, True]),
        )
        if setting not in contract.settings:
            raise ValueError("trial is outside the frozen experiment")
        return evaluate(setting)

    if remaining:
        study.optimize(objective, n_trials=remaining, n_jobs=1)
    return study
