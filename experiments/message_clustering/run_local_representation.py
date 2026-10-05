"""Score six declared Optuna trials, preserving independent diagnostic labels."""

import hashlib
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from ablation_evidence import fit, fuse
from analyze_link_review import analyze, load_labels
from build_local_representation import Artifact, name, pinned
from corpus import Record
from experiment_study import Context, Contract, Setting, run
from freeze_link_review import SOURCE_SHA, ReviewManifest
from run_study_development import Interval, bootstrap
from run_turn_ablation import AblationReport, matrix
from study_annotations import load_development
from study_scoring import JudgedPair, absolute_gate, combine, minimum_gain, score
from temporal_smoothing import Setting as Smoothing
from temporal_smoothing import lineages, smooth
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase


class Row(Record):
    thread_index: int
    long: bool
    counts: Counts
    diagnostic: Counts
    review_sensitivity: Counts
    predictions: dict[str, bool]
    seconds: float


class Result(Record):
    setting: Setting
    rows: tuple[Row, ...]
    overall: Counts
    long: Counts
    diagnostic: Counts
    review_sensitivity: Counts
    interval: Interval
    long_interval: Interval
    singleton_interval: Interval
    eligible_quality: bool
    fit_p95_seconds: float
    changed_benchmark_predictions: int
    artifact_sha256: dict[str, str]
    confirmation: str = "sealed"


def score_setting(
    setting: Setting,
    parent: Path,
    artifacts: Path,
    contract: Contract,
    control: tuple[Row, ...] | None = None,
) -> Result:
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    if hashlib.sha256(cohort_raw).hexdigest() != contract.cohort_sha256:
        raise ValueError("cohort changed")
    cohort = Cohort.model_validate_json(cohort_raw)
    report_raw = (
        parent / "turn-three-factor-ablation-20261006/development.json"
    ).read_bytes()
    if hashlib.sha256(report_raw).hexdigest() != SOURCE_SHA:
        raise ValueError("frozen report changed")
    report = AblationReport.model_validate_json(report_raw)
    labels = load_development(parent / "turn-study-blind-20261005")
    if (
        labels.manifest_sha256 != report.report.annotation_manifest_sha256
        or labels.file_sha256 != report.report.annotation_sha256
    ):
        raise ValueError("original frozen annotations changed")
    review_root = parent / "turn-link-review-20261006/packets"
    review_audit = analyze(review_root)
    if (
        review_audit.manifest_sha256
        != "95903c86c5f10a2e1645a13f3b9c0cb444ecfd44e7c45c741988507e2fd6b69a"
    ):
        raise ValueError("blind diagnostic sample changed")
    manifest = ReviewManifest.model_validate_json(
        (review_root / "manifest.json").read_bytes()
    )
    diagnostic, _ = load_labels(manifest.primary_packets)
    secondary, _ = load_labels(manifest.review_packets)
    baseline = {r.thread_index: r for r in report.report.rows if r.name == "c0f0g0"}
    chronological = combine(
        tuple(r.counts for r in report.report.rows if r.name == "chronological_4")
    )
    rows = []
    hashes = {}
    changed = 0
    with threadpool_limits(limits=1):
        for entry in cohort.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            captured = entry.captured
            path = artifacts / (name(captured.index, setting) + ".json")
            raw = path.read_bytes()
            built = Artifact.model_validate_json(raw)
            vector_raw = path.with_suffix(".npz").read_bytes()
            if (
                built.protocol_sha256 != contract.protocol_sha256
                or built.cohort_sha256 != contract.cohort_sha256
                or built.encoder_generation != contract.encoder_generation
                or built.setting != setting
                or built.thread_index != captured.index
                or hashlib.sha256(vector_raw).hexdigest() != built.matrix_sha256
            ):
                raise ValueError("trial artifact provenance differs")
            hashes[path.name] = hashlib.sha256(raw).hexdigest()
            hashes[path.with_suffix(".npz").name] = built.matrix_sha256
            b_vectors, b_owners = matrix(vector_raw)
            if b_owners != tuple(u.body.turn_id for u in built.units):
                raise ValueError("matrix point ownership changed")
            filename = f"{captured.index}-context_whole.npz"
            a_vectors, a_owners = matrix(
                pinned(
                    parent / "turn-input-followup-20261005/artifacts" / filename,
                    f"pooled/{filename}",
                    report,
                )
            )
            started = time.perf_counter()
            a = fit(
                smooth(
                    a_vectors,
                    a_owners,
                    lineages(captured.thread, captured.turns),
                    Smoothing(alpha=0.10, window=4),
                ),
                a_owners,
            )
            b = fit(b_vectors, b_owners)
            members = fuse(a, b, (0.0,) * len(b_owners), False)
            seconds = time.perf_counter() - started
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
            base_predictions = {
                p.pair_id: p.overlapping for p in baseline[captured.index].predictions
            }
            changed += sum(v != base_predictions[k] for k, v in predictions.items())
            extra_refs = tuple(
                r for r in manifest.references if r.thread_index == captured.index
            )
            extra = tuple(
                JudgedPair(
                    r.draw.pair_id,
                    r.a_turn_id,
                    r.b_turn_id,
                    diagnostic[r.draw.pair_id].verdict,
                )
                for r in extra_refs
            )
            alternative = tuple(
                JudgedPair(
                    p.id,
                    p.a,
                    p.b,
                    secondary[p.id].verdict if p.id in secondary else p.verdict,
                )
                for p in extra
            )
            extra_predictions = {p.id: bool(members[p.a] & members[p.b]) for p in extra}
            rows.append(
                Row(
                    thread_index=captured.index,
                    long=len(a_owners) >= 8,
                    counts=score(pairs, predictions),
                    diagnostic=score(extra, extra_predictions),
                    review_sensitivity=score(alternative, extra_predictions),
                    predictions=predictions,
                    seconds=seconds,
                )
            )
    overall = combine(tuple(r.counts for r in rows))
    long_rows = tuple(r for r in rows if r.long)
    long = combine(tuple(r.counts for r in long_rows))
    control_rows = control if control is not None else tuple(rows)
    if tuple(r.thread_index for r in control_rows) != tuple(
        r.thread_index for r in rows
    ):
        raise ValueError("singleton control history identities differ")
    return Result(
        setting=setting,
        rows=tuple(rows),
        overall=overall,
        long=long,
        diagnostic=combine(tuple(r.diagnostic for r in rows)),
        review_sensitivity=combine(tuple(r.review_sensitivity for r in rows)),
        interval=bootstrap(
            tuple(r.counts for r in rows),
            tuple(baseline[r.thread_index].counts for r in rows),
        ),
        long_interval=bootstrap(
            tuple(r.counts for r in long_rows),
            tuple(baseline[r.thread_index].counts for r in long_rows),
        ),
        singleton_interval=bootstrap(
            tuple(r.counts for r in rows), tuple(r.counts for r in control_rows)
        ),
        eligible_quality=(
            absolute_gate(overall)
            and absolute_gate(long)
            and minimum_gain(overall, chronological)
        ),
        fit_p95_seconds=float(np.quantile([r.seconds for r in rows], 0.95)),
        changed_benchmark_predictions=changed,
        artifact_sha256=hashes,
    )


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(parent: Path, artifacts: Path, output: Path) -> None:
    os.umask(0o077)
    contract = Contract.model_validate_json((artifacts / "contract.json").read_bytes())
    output.mkdir(mode=0o700, parents=True, exist_ok=True)

    def evaluate(setting: Setting) -> float:
        control_path = output / "none-0.json"
        control = (
            None
            if setting.context == Context.NONE and not setting.boilerplate
            else Result.model_validate_json(control_path.read_bytes()).rows
        )
        result = score_setting(setting, parent, artifacts, contract, control)
        path = output / f"{setting.context.value}-{int(setting.boilerplate)}.json"
        path.write_bytes(orjson.dumps(result.model_dump(mode="json")))
        click.echo(
            f"{setting.context.value}/{setting.boilerplate}: F1={result.overall.f1:.6f}"
        )
        return result.overall.f1

    study = run(
        output / "optuna.sqlite", "thread-local-representation", contract, evaluate
    )
    # Completed trials require their independently verifiable report, even on resume.
    for setting in contract.settings:
        result = Result.model_validate_json(
            (
                output / f"{setting.context.value}-{int(setting.boilerplate)}.json"
            ).read_bytes()
        )
        if result.setting != setting or len(result.rows) != 160:
            raise ValueError("completed trial report missing/incomplete")
    click.echo(f"Completed {len(study.trials)} declared trials; confirmation sealed")


if __name__ == "__main__":
    main()
