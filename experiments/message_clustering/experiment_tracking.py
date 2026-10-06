"""Local MLflow tracking of aggregate experiment results, without chat payloads."""

import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

import orjson
from mlflow.entities import Metric, Param
from mlflow.tracking import MlflowClient

from corpus import Record
from run_complete_answers import Result


class Summary(Record):
    baseline: str = "complete-answer-v1"
    treatment: str
    confirmation: str = "sealed"
    metrics: dict[str, float]
    provenance: dict[str, str]


def metrics(result: Result) -> dict[str, float]:
    values = {}
    for prefix, counts in (("overall", result.overall), ("long", result.long)):
        for key in ("tp", "fp", "fn", "tn", "precision", "recall", "f1"):
            values[f"{prefix}.{key}"] = float(getattr(counts, key))
    for prefix, interval in (
        ("overall", result.interval),
        ("long", result.long_interval),
    ):
        values[f"{prefix}.f1_gain_low"] = interval.gain_low
        values[f"{prefix}.f1_gain_high"] = interval.gain_high
    distant_total = sum(row.distant_long_related for row in result.rows)
    distant_missed = sum(row.distant_long_missed for row in result.rows)
    values.update(
        {
            "distant_long.recall": (distant_total - distant_missed) / distant_total
            if distant_total
            else 0.0,
            "long.misses_recovered": float(result.long_misses_recovered),
            "long.misses_introduced": float(result.long_misses_introduced),
            "predictions_changed": float(result.changed_predictions),
            "fit_seconds": sum(row.seconds for row in result.rows),
            "passes_absolute_quality": float(result.passes_absolute_quality),
        }
    )
    return values


@contextmanager
def tracked_run(
    root: Path, study: str, name: str, parameters: dict[str, str]
) -> Iterator[tuple[MlflowClient, str]]:
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    artifacts = root / "artifacts"
    artifacts.mkdir(mode=0o700, exist_ok=True)
    client = MlflowClient(
        tracking_uri=f"sqlite:///{(root / 'tracking.sqlite').resolve()}"
    )
    experiment = client.get_experiment_by_name(study)
    experiment_id = (
        experiment.experiment_id
        if experiment
        else client.create_experiment(
            study, artifact_location=artifacts.resolve().as_uri()
        )
    )
    run = client.create_run(
        experiment_id, tags={"mlflow.runName": name, "confirmation": "sealed"}
    )
    run_id = run.info.run_id
    try:
        client.log_batch(
            run_id, params=[Param(key, value) for key, value in parameters.items()]
        )
        yield client, run_id
    except BaseException:
        client.set_terminated(run_id, status="FAILED")
        raise
    else:
        client.set_terminated(run_id, status="FINISHED")


def log_summary(client: MlflowClient, run_id: str, summary: Summary) -> None:
    timestamp = int(time.time() * 1000)
    client.log_batch(
        run_id,
        metrics=[
            Metric(key, value, timestamp, 0) for key, value in summary.metrics.items()
        ],
    )
    client.log_text(
        run_id, orjson.dumps(summary.model_dump(mode="json")).decode(), "summary.json"
    )
