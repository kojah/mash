"""Score the registered development grid and freeze one choice before confirmation."""

import hashlib
import io
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from blind_turn_pairs import Stratum, Verdict
from corpus import Record
from study_annotations import Agreement, load_development
from study_artifacts import View
from study_scoring import (
    CANDIDATES,
    Decision,
    Family,
    JudgedPair,
    Summary,
    choose,
    combine,
    connections,
    fit,
    lexical_baselines,
    score,
)
from turn_evaluation import Counts
from turn_study_corpus import Cohort, Phase


class Prediction(Record):
    pair_id: str
    stratum: Stratum
    batch: int = 0
    overlapping: bool
    primary: bool


class Row(Record):
    name: str
    thread_index: int
    provider: str
    node: str
    prompted_turns: int
    counts: Counts
    primary_counts: Counts
    seconds: float
    group_sizes: tuple[int, ...] = ()
    assigned_turns: int | None = None
    predictions: tuple[Prediction, ...] = ()


class Interval(Record):
    f1_low: float
    f1_high: float
    gain_low: float
    gain_high: float


class Report(Record):
    cohort_sha256: str
    protocol_sha256: str
    annotation_sha256: dict[str, str]
    annotation_manifest_sha256: str
    artifact_sha256: dict[str, str]
    encoder_manifest_sha256: str
    agreement: Agreement
    candidates: tuple[Summary, ...]
    baselines: tuple[Summary, ...]
    decision: Decision
    selected_interval: Interval | None
    rows: tuple[Row, ...]


def bootstrap(selected: tuple[Counts, ...], baseline: tuple[Counts, ...]) -> Interval:
    """Paired thread bootstrap, including negative-only and unjudged histories."""
    if len(selected) != len(baseline) or not selected:
        raise ValueError("bootstrap requires matched nonempty thread counts")
    rng = np.random.default_rng(20261005)
    indices = rng.integers(0, len(selected), size=(2000, len(selected)))

    def f1(values: tuple[Counts, ...]) -> np.ndarray:
        matrix = np.array([(c.tp, c.fp, c.fn) for c in values])
        totals = matrix[indices].sum(axis=1)
        denominator = 2 * totals[:, 0] + totals[:, 1] + totals[:, 2]
        return np.divide(
            2 * totals[:, 0],
            denominator,
            out=np.zeros(len(indices)),
            where=denominator != 0,
        )

    scores = f1(selected)
    low, high = np.quantile(scores, (0.025, 0.975))
    gain_low, gain_high = np.quantile(scores - f1(baseline), (0.025, 0.975))
    return Interval(f1_low=low, f1_high=high, gain_low=gain_low, gain_high=gain_high)


def summarize(rows: tuple[Row, ...], name: str) -> Summary:
    selected = tuple(row for row in rows if row.name == name)
    if not selected:
        raise ValueError("cannot summarize absent configuration")
    return Summary(
        name=name,
        counts=combine(tuple(row.counts for row in selected)),
        seconds=sum(row.seconds for row in selected),
    )


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path, protocol: Path, annotations: Path, artifacts: Path, output: Path
) -> None:
    os.umask(0o077)
    if output.exists():
        raise click.ClickException(
            "development result already exists; do not overwrite"
        )
    raw = cohort.read_bytes()
    data = Cohort.model_validate_json(raw)
    cohort_sha = hashlib.sha256(raw).hexdigest()
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    labels = load_development(annotations)
    if (
        labels.manifest.cohort_sha256 != cohort_sha
        or labels.manifest.protocol_sha256 != protocol_sha
        or (artifacts / "cohort.sha256").read_text() != cohort_sha
        or (artifacts / "protocol.sha256").read_text() != protocol_sha
    ):
        raise click.ClickException("cohort/protocol provenance mismatch")
    verdicts = tuple(label.verdict for label in labels.labels.values())
    if (
        verdicts.count(Verdict.RELATED) < 100
        or verdicts.count(Verdict.UNRELATED) < 200
        or not labels.confirmation_adequate
    ):
        raise click.ClickException("registered label adequacy gate is not satisfied")
    missing = tuple(
        f"{entry.captured.index}-{view.value}.npz"
        for entry in data.entries
        if entry.phase == Phase.DEVELOPMENT
        for view in View
        if not (artifacts / f"{entry.captured.index}-{view.value}.npz").is_file()
    )
    if missing:
        raise click.ClickException(
            f"artifact build incomplete: {len(missing)} development matrices missing"
        )
    rows: list[Row] = []
    artifact_sha256: dict[str, str] = {}
    baseline_names = [
        "no_cross_turn",
        "chronological_2",
        "chronological_4",
        "tfidf_0.65",
        "tfidf_0.75",
        "tfidf_0.85",
    ]
    baseline_names.extend(
        c.name
        for c in CANDIDATES
        if c.view == View.PROMPT and c.family == Family.LEADER
    )
    with threadpool_limits(limits=1):
        for entry in data.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            captured = entry.captured
            turns = tuple(turn for turn in captured.turns if turn.prompts)
            owners = tuple(turn.id for turn in turns)
            references = tuple(
                ref
                for ref in labels.manifest.references
                if ref.thread_index == captured.index and ref.phase == Phase.DEVELOPMENT
            )
            pairs = tuple(
                JudgedPair(
                    ref.pair_id,
                    ref.a_turn_id,
                    ref.b_turn_id,
                    labels.labels[ref.pair_id].verdict,
                )
                for ref in references
            )
            prompts = tuple(
                "\n".join(captured.thread.messages[i].projection for i in turn.prompts)
                for turn in turns
            )
            baseline_started = time.perf_counter()
            baseline_predictions = lexical_baselines(prompts, owners, pairs)
            # These controls share one TF-IDF construction; report amortized cost.
            baseline_seconds = (time.perf_counter() - baseline_started) / len(
                baseline_predictions
            )
            for name, predictions in baseline_predictions.items():
                counts = score(pairs, predictions)
                rows.append(
                    Row(
                        name=name,
                        thread_index=captured.index,
                        provider=captured.thread.provider.value,
                        node=entry.node,
                        prompted_turns=len(turns),
                        counts=counts,
                        primary_counts=counts,
                        seconds=baseline_seconds,
                        predictions=tuple(
                            Prediction(
                                pair_id=ref.pair_id,
                                stratum=ref.stratum,
                                batch=ref.batch,
                                overlapping=predictions[ref.pair_id],
                                primary=predictions[ref.pair_id],
                            )
                            for ref in references
                        ),
                    )
                )
            for view in View:
                name = f"{captured.index}-{view.value}.npz"
                matrix_bytes = (artifacts / name).read_bytes()
                artifact_sha256[name] = hashlib.sha256(matrix_bytes).hexdigest()
                with np.load(io.BytesIO(matrix_bytes), allow_pickle=False) as points:
                    vectors = points["vectors"]
                    point_owners = tuple(str(value) for value in points["turn_ids"])
                if set(point_owners) != set(owners):
                    raise click.ClickException("artifact turn identities mismatch")
                for candidate in CANDIDATES:
                    if candidate.view != view:
                        continue
                    assigned = fit(vectors, point_owners, candidate)
                    overlapping = connections(pairs, assigned.overlapping)
                    primary = connections(pairs, assigned.primary)
                    rows.append(
                        Row(
                            name=candidate.name,
                            thread_index=captured.index,
                            provider=captured.thread.provider.value,
                            node=entry.node,
                            prompted_turns=len(turns),
                            counts=score(pairs, overlapping),
                            primary_counts=score(pairs, primary),
                            seconds=assigned.seconds,
                            group_sizes=assigned.group_sizes,
                            assigned_turns=sum(
                                bool(groups) for groups in assigned.overlapping.values()
                            ),
                            predictions=tuple(
                                Prediction(
                                    pair_id=ref.pair_id,
                                    stratum=ref.stratum,
                                    batch=ref.batch,
                                    overlapping=overlapping[ref.pair_id],
                                    primary=primary[ref.pair_id],
                                )
                                for ref in references
                            ),
                        )
                    )
            click.echo(f"Scored development thread {captured.index}")
    frozen_rows = tuple(rows)
    candidates = tuple(
        summarize(frozen_rows, candidate.name) for candidate in CANDIDATES
    )
    baselines = tuple(summarize(frozen_rows, name) for name in baseline_names)
    decision = choose(candidates, baselines)
    interval = (
        bootstrap(
            tuple(row.counts for row in rows if row.name == decision.selected),
            tuple(row.counts for row in rows if row.name == decision.baseline),
        )
        if decision.selected
        else None
    )
    report = Report(
        cohort_sha256=cohort_sha,
        protocol_sha256=protocol_sha,
        annotation_sha256=labels.file_sha256,
        annotation_manifest_sha256=labels.manifest_sha256,
        artifact_sha256=artifact_sha256,
        encoder_manifest_sha256=hashlib.sha256(
            (artifacts / "encoder.json").read_bytes()
        ).hexdigest(),
        agreement=labels.agreement,
        candidates=candidates,
        baselines=baselines,
        decision=decision,
        selected_interval=interval,
        rows=frozen_rows,
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as destination:
        destination.write(orjson.dumps(report.model_dump(mode="json")))
    click.echo(
        f"Frozen development choice: {decision.selected}; baseline={decision.baseline}"
    )


if __name__ == "__main__":
    main()
