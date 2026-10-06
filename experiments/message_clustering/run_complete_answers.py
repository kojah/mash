"""Score the two frozen coverage controls without changing the clustering baseline."""

import io
import os
import time
from pathlib import Path

import click
import numpy as np
import optuna
import orjson
from optuna.trial import TrialState
from threadpoolctl import threadpool_limits

from ablation_evidence import fit, fuse
from audit_baseline_failures import evidence_covered
from blind_turn_pairs import Verdict
from build_complete_answers import Artifact, Contract, name
from build_token_weighting import digest
from corpus import Record
from followup_inputs import AnswerScope
from run_study_development import Interval, bootstrap
from run_token_weighting import Result as ParentControl
from study_annotations import load_development
from study_scoring import JudgedPair, absolute_gate, combine, score
from temporal_smoothing import Setting, lineages, smooth
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase


class Row(Record):
    thread_index: int
    long: bool
    counts: Counts
    predictions: dict[str, bool]
    distant_long_related: int
    distant_long_missed: int
    omitted_quote_pairs: int
    unknown_quote_pairs: int
    seconds: float


class Result(Record):
    scope: AnswerScope
    contract_sha256: str
    rows: tuple[Row, ...]
    overall: Counts
    long: Counts
    interval: Interval
    long_interval: Interval
    long_misses_recovered: int
    long_misses_introduced: int
    changed_predictions: int
    passes_absolute_quality: bool
    artifact_sha256: dict[str, str]
    annotation_sha256: dict[str, str]
    code_sha256: dict[str, str]
    confirmation: str = "sealed"


def evaluate(
    parent: Path, artifacts: Path, scope: AnswerScope, control: Result | None
) -> Result:
    raw = (artifacts / "contract.json").read_bytes()
    contract = Contract.model_validate_json(raw)
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    if digest(cohort_raw) != contract.cohort_sha256:
        raise ValueError("cohort changed")
    cohort = Cohort.model_validate_json(cohort_raw)
    labels = load_development(parent / "turn-study-blind-20261005")
    rows = []
    hashes = {}
    with threadpool_limits(limits=1):
        for entry in cohort.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            captured = entry.captured
            path = artifacts / (name(captured.index, scope) + ".json")
            source_raw = path.read_bytes()
            built = Artifact.model_validate_json(source_raw)
            matrix_raw = path.with_suffix(".npz").read_bytes()
            if (
                built.scope != scope
                or built.thread_index != captured.index
                or built.contract_sha256 != digest(raw)
                or built.matrix_sha256 != digest(matrix_raw)
                or built.source_sha256
                != digest(orjson.dumps(captured.model_dump(mode="json")))
            ):
                raise ValueError("coverage artifact provenance changed")
            hashes[path.name] = digest(source_raw)
            hashes[path.with_suffix(".npz").name] = digest(matrix_raw)
            with np.load(io.BytesIO(matrix_raw), allow_pickle=False) as archive:
                a = archive["pooled"]
                b = archive["chunks"]
                a_owners = tuple(archive["pooled_owners"].tolist())
                b_owners = tuple(archive["chunk_owners"].tolist())
            if b_owners != tuple(i.turn_id for i in built.chunk_inputs):
                raise ValueError("chunk ownership changed")
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
            fit_seconds = time.perf_counter() - started
            refs = tuple(
                r
                for r in labels.manifest.references
                if r.thread_index == captured.index and r.pair_id in labels.labels
            )
            pairs = tuple(
                JudgedPair(
                    r.pair_id,
                    r.a_turn_id,
                    r.b_turn_id,
                    labels.labels[r.pair_id].verdict,
                )
                for r in refs
            )
            predictions = {p.id: bool(members[p.a] & members[p.b]) for p in pairs}
            long = len(a_owners) >= 8
            distant = tuple(
                p
                for p in pairs
                if long
                and p.verdict == Verdict.RELATED
                and abs(a_owners.index(p.a) - a_owners.index(p.b)) >= 4
            )
            turns = {t.id: t for t in captured.turns}
            coverage = []
            for ref in refs:
                label = labels.labels[ref.pair_id]
                if label.verdict == Verdict.AMBIGUOUS:
                    continue
                coverage.append(
                    tuple(
                        evidence_covered(
                            captured.thread,
                            turns[identity],
                            quote,
                            built.pooled_inputs + built.chunk_inputs,
                        )
                        for identity, quote in (
                            (ref.a_turn_id, label.evidence_a),
                            (ref.b_turn_id, label.evidence_b),
                        )
                    )
                )
            rows.append(
                Row(
                    thread_index=captured.index,
                    long=long,
                    counts=score(pairs, predictions),
                    predictions=predictions,
                    distant_long_related=len(distant),
                    distant_long_missed=sum(not predictions[p.id] for p in distant),
                    omitted_quote_pairs=sum(False in pair for pair in coverage),
                    unknown_quote_pairs=sum(None in pair for pair in coverage),
                    seconds=fit_seconds,
                )
            )
    rows = tuple(rows)
    baseline = control.rows if control else rows
    if tuple(r.thread_index for r in baseline) != tuple(r.thread_index for r in rows):
        raise ValueError("matched control identities differ")
    if scope == AnswerScope.LAST:
        prior = ParentControl.model_validate_json(
            (parent / "token-downweighting-20261006/results/uniform.json").read_bytes()
        )
        if tuple((r.thread_index, r.counts, r.predictions) for r in rows) != tuple(
            (r.thread_index, r.counts, r.predictions) for r in prior.rows
        ):
            raise ValueError("matched last-only predictions differ from prior uniform")
    overall = combine(tuple(r.counts for r in rows))
    long = combine(tuple(r.counts for r in rows if r.long))
    verdicts = {
        p.pair_id: labels.labels[p.pair_id].verdict
        for p in labels.manifest.references
        if p.pair_id in labels.labels
    }
    return Result(
        scope=scope,
        contract_sha256=digest(raw),
        rows=rows,
        overall=overall,
        long=long,
        interval=bootstrap(
            tuple(r.counts for r in rows), tuple(r.counts for r in baseline)
        ),
        long_interval=bootstrap(
            tuple(r.counts for r in rows if r.long),
            tuple(r.counts for r in baseline if r.long),
        ),
        long_misses_recovered=sum(
            not old.predictions[k] and prediction
            for new, old in zip(rows, baseline, strict=True)
            if new.long
            for k, prediction in new.predictions.items()
            if verdicts[k] == Verdict.RELATED
        ),
        long_misses_introduced=sum(
            old.predictions[k] and not prediction
            for new, old in zip(rows, baseline, strict=True)
            if new.long
            for k, prediction in new.predictions.items()
            if verdicts[k] == Verdict.RELATED
        ),
        changed_predictions=sum(
            p != old.predictions[k]
            for new, old in zip(rows, baseline, strict=True)
            for k, p in new.predictions.items()
        ),
        passes_absolute_quality=absolute_gate(overall) and absolute_gate(long),
        artifact_sha256=hashes,
        annotation_sha256=labels.file_sha256,
        code_sha256={
            n: digest(Path(__file__).with_name(n).read_bytes())
            for n in (
                "run_complete_answers.py",
                "ablation_evidence.py",
                "temporal_smoothing.py",
                "audit_baseline_failures.py",
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
    raw = (artifacts / "contract.json").read_bytes()
    contract = Contract.model_validate_json(raw)
    study = optuna.create_study(
        study_name="complete-answer-baseline",
        storage=f"sqlite:///{(output / 'optuna.sqlite').resolve()}",
        direction="maximize",
        load_if_exists=True,
        sampler=optuna.samplers.RandomSampler(seed=0),
    )
    metadata = contract.model_dump(mode="json")
    if study.user_attrs.get("contract", metadata) != metadata:
        raise ValueError("study contract changed")
    if study.trials and "contract" not in study.user_attrs:
        raise ValueError("existing trials lack pinned contract")
    if any(t.state == TrialState.RUNNING for t in study.trials):
        raise ValueError("interrupted/live trial requires explicit recovery")
    study.set_user_attr("contract", metadata)
    scheduled = {
        t.params.get("answer_scope", t.user_attrs.get("answer_scope"))
        for t in study.trials
        if t.state in (TrialState.COMPLETE, TrialState.WAITING)
    }
    for scope in contract.scopes:
        if scope.value not in scheduled:
            study.enqueue_trial(
                {"answer_scope": scope.value}, user_attrs={"answer_scope": scope.value}
            )

    def objective(trial: optuna.Trial) -> float:
        scope = AnswerScope(
            trial.suggest_categorical(
                "answer_scope", [s.value for s in contract.scopes]
            )
        )
        control = (
            None
            if scope == AnswerScope.LAST
            else Result.model_validate_json((output / "last.json").read_bytes())
        )
        result = evaluate(parent, artifacts, scope, control)
        (output / f"{scope}.json").write_bytes(
            orjson.dumps(result.model_dump(mode="json"))
        )
        click.echo(
            f"{scope}: F1={result.overall.f1:.6f} long_R={result.long.recall:.6f}"
        )
        return result.overall.f1

    study.optimize(
        objective,
        n_trials=sum(t.state == TrialState.WAITING for t in study.trials),
        n_jobs=1,
    )
    for scope in contract.scopes:
        result = Result.model_validate_json((output / f"{scope}.json").read_bytes())
        if (
            result.scope != scope
            or result.contract_sha256 != digest(raw)
            or len(result.rows) != 160
        ):
            raise ValueError("completed trial report is missing or changed")


if __name__ == "__main__":
    main()
