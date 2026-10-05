"""Consolidate declared contrasts, inference cost and verified audit evidence."""

import hashlib
import os
import re
import sqlite3
from pathlib import Path

import click
import numpy as np
import orjson
from optuna import load_study
from optuna.trial import TrialState

from audit_local_representation import Audit
from build_local_representation import Artifact, name
from corpus import Record
from experiment_study import Context, Contract, Setting
from run_local_representation import Result
from run_study_development import Interval, bootstrap
from turn_evaluation import Counts


class Cost(Record):
    name: str
    points: int
    weighted_points: int
    weighted_tokens: int
    accepted_contexts: int
    construction_median: float
    construction_p95: float
    construction_max: float
    fit_p95: float
    performance_gates: bool


class Contrast(Record):
    treatment: str
    control: str
    precision_gain: float
    recall_gain: float
    f1_gain: float
    long_precision_gain: float
    long_recall_gain: float
    long_f1_gain: float
    changed_predictions: int
    interval: Interval
    long_interval: Interval


class Scores(Record):
    name: str
    overall: Counts
    long: Counts
    diagnostic: Counts
    review_sensitivity: Counts
    quality_gate: bool
    historical_f1_change_interval: Interval


class Summary(Record):
    contract: Contract
    source_sha256: dict[str, str]
    audit: Audit
    scores: tuple[Scores, ...]
    costs: tuple[Cost, ...]
    contrasts: tuple[Contrast, ...]
    unique_new_vectors: int
    optuna_completed: int
    peak_rss_kib: int
    memory_gate: bool
    confirmation: str = "sealed"
    limitation: str = (
        "Repeated development evaluation is exploratory. Bootstrap intervals are "
        "unadjusted, not confirmatory significance tests. Blind targeted diagnostic "
        "counts are not population estimates. Historical pooled/user vectors remain "
        "fixed own-history inputs; only new chunk/candidate inference is singleton. "
        "Construction timings mix cold and resumed cache work under host I/O "
        "contention; they are observed durations, not an isolated cold benchmark."
    )


def label(setting: Setting) -> str:
    return f"{setting.context.value}-{int(setting.boilerplate)}"


def contrast(treatment: Result, control: Result) -> Contrast:
    if tuple(r.thread_index for r in treatment.rows) != tuple(
        r.thread_index for r in control.rows
    ):
        raise ValueError("contrast history identities differ")
    long_t = tuple(r.counts for r in treatment.rows if r.long)
    long_c = tuple(r.counts for r in control.rows if r.long)
    return Contrast(
        treatment=label(treatment.setting),
        control=label(control.setting),
        precision_gain=treatment.overall.precision - control.overall.precision,
        recall_gain=treatment.overall.recall - control.overall.recall,
        f1_gain=treatment.overall.f1 - control.overall.f1,
        long_precision_gain=treatment.long.precision - control.long.precision,
        long_recall_gain=treatment.long.recall - control.long.recall,
        long_f1_gain=treatment.long.f1 - control.long.f1,
        changed_predictions=sum(
            a.predictions[k] != b.predictions[k]
            for a, b in zip(treatment.rows, control.rows, strict=True)
            for k in a.predictions
        ),
        interval=bootstrap(
            tuple(r.counts for r in treatment.rows),
            tuple(r.counts for r in control.rows),
        ),
        long_interval=bootstrap(long_t, long_c),
    )


@click.command()
@click.option("--root", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(root: Path, output: Path) -> None:
    os.umask(0o077)
    artifacts = root / "artifacts-singleton"
    contract = Contract.model_validate_json((artifacts / "contract.json").read_bytes())
    audit = Audit.model_validate_json((root / "audit.json").read_bytes())
    if (
        audit.histories != 160
        or audit.artifacts != 960
        or audit.isolation_settings_checked != 24
    ):
        raise ValueError("required artifact/isolation audit incomplete")
    results = {}
    costs = []
    scores = []
    hashes = {}
    for setting in contract.settings:
        path = root / "results" / (label(setting) + ".json")
        raw = path.read_bytes()
        result = Result.model_validate_json(raw)
        if (
            result.setting != setting
            or len(result.rows) != 160
            or result.confirmation != "sealed"
        ):
            raise ValueError("trial coverage/settings changed")
        for filename, expected in result.artifact_sha256.items():
            if (
                hashlib.sha256((artifacts / filename).read_bytes()).hexdigest()
                != expected
            ):
                raise ValueError("scored artifact changed")
        hashes[str(path.relative_to(root))] = hashlib.sha256(raw).hexdigest()
        results[label(setting)] = result
        built = tuple(
            Artifact.model_validate_json(
                (artifacts / (name(r.thread_index, setting) + ".json")).read_bytes()
            )
            for r in result.rows
        )
        times = [a.seconds for a in built]
        costs.append(
            Cost(
                name=label(setting),
                points=sum(len(a.units) for a in built),
                weighted_points=sum(a.weighted_points for a in built),
                weighted_tokens=sum(a.weighted_tokens for a in built),
                accepted_contexts=sum(a.accepted_contexts for a in built),
                construction_median=float(np.median(times)),
                construction_p95=float(np.quantile(times, 0.95)),
                construction_max=max(times),
                fit_p95=result.fit_p95_seconds,
                performance_gates=float(np.quantile(times, 0.95)) <= 300
                and result.fit_p95_seconds <= 10,
            )
        )
        scores.append(
            Scores(
                name=label(setting),
                overall=result.overall,
                long=result.long,
                diagnostic=result.diagnostic,
                review_sensitivity=result.review_sensitivity,
                quality_gate=result.eligible_quality,
                historical_f1_change_interval=result.interval,
            )
        )
    # All prospective weighting contrasts plus context comparisons, no new trials.
    comparisons = [(f"{c.value}-1", f"{c.value}-0") for c in Context]
    comparisons += [
        ("first80-0", "none-0"),
        ("selected80-0", "none-0"),
        ("selected80-0", "first80-0"),
    ]
    fingerprint = contract.encoder_generation + ":singleton-v1"
    generations = (
        fingerprint,
        hashlib.sha256(
            (fingerprint + ":character-overlap-pooling-v1").encode()
        ).hexdigest(),
        hashlib.sha256((fingerprint + ":weighted-pooling-4-1-v1").encode()).hexdigest(),
    )
    db = sqlite3.connect(
        (artifacts / "vectors.sqlite").resolve().as_uri() + "?mode=ro", uri=True
    )
    try:
        unique = db.execute(
            "SELECT COUNT(*) FROM vectors WHERE generation IN (?,?,?)", generations
        ).fetchone()[0]
    finally:
        db.close()
    study = load_study(
        study_name="thread-local-representation",
        storage=f"sqlite:///{(root / 'results/optuna.sqlite').resolve()}",
    )
    if len(study.trials) != 6 or any(
        t.state != TrialState.COMPLETE for t in study.trials
    ):
        raise ValueError("Optuna trial ledger incomplete")
    for trial, setting in zip(study.trials, contract.settings, strict=True):
        if (
            trial.params != setting.model_dump(mode="json")
            or trial.value != results[label(setting)].overall.f1
        ):
            raise ValueError("Optuna trial differs from independently scored result")
    hashes["audit.json"] = hashlib.sha256(
        (root / "audit.json").read_bytes()
    ).hexdigest()
    hashes["protocol.md"] = hashlib.sha256(
        (root / "protocol.md").read_bytes()
    ).hexdigest()
    if hashes["protocol.md"] != contract.protocol_sha256:
        raise ValueError("preregistered protocol changed")
    measurements = []
    for filename in (
        "build-singleton-before-write-batching-time.txt",
        "build-singleton-resume-time.txt",
        "audit-time.txt",
        "score-time.txt",
    ):
        raw = (root / filename).read_bytes()
        match = re.search(rb"Maximum resident set size \(kbytes\):\s*(\d+)", raw)
        if match is None:
            raise ValueError("runtime measurement missing peak RSS")
        measurements.append(int(match[1]))
        hashes[filename] = hashlib.sha256(raw).hexdigest()
    output.write_bytes(
        orjson.dumps(
            Summary(
                contract=contract,
                source_sha256=hashes,
                audit=audit,
                scores=tuple(scores),
                costs=tuple(costs),
                contrasts=tuple(
                    contrast(results[t], results[c]) for t, c in comparisons
                ),
                unique_new_vectors=unique,
                optuna_completed=len(study.trials),
                peak_rss_kib=max(measurements),
                memory_gate=max(measurements) <= 4 * 1024 * 1024,
            ).model_dump(mode="json")
        )
    )
    click.echo("Consolidated six verified trials; confirmation sealed")


if __name__ == "__main__":
    main()
