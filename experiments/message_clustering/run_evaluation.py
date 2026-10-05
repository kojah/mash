"""Development or frozen-held-out comparison, with private output only."""

import hashlib
import os
import time
from dataclasses import asdict
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from clustering import CONFIGURATIONS, Group, Membership, Result, cluster
from corpus import Record, Split
from evaluation import PairMetrics, evaluate_pairs
from frozen_settings import FrozenSettings
from preparation import Prepared, prepare
from references import Case, Kind, ThreadReference, load_references


class Score(Record):
    precision: float
    recall: float
    f1: float
    tp: int
    fp: int
    fn: int
    tn: int
    ambiguous: int


def score(metrics: PairMetrics) -> Score:
    return Score(
        precision=metrics.precision,
        recall=metrics.recall,
        f1=metrics.f1,
        tp=metrics.true_positive,
        fp=metrics.false_positive,
        fn=metrics.false_negative,
        tn=metrics.true_negative,
        ambiguous=metrics.ambiguous,
    )


class ThreadResult(Record):
    thread_hash: str
    provider: str
    length: str
    source_messages: int
    units: int
    groups: int
    unreviewed_assumed_substantive: int
    reviewed_substantive: int
    assigned_substantive: int
    seconds: float
    partition: Score
    overlapping: Score
    cases: dict[Case, Score]


class CandidateResult(Record):
    configuration: str
    threads: tuple[ThreadResult, ...]


class Report(Record):
    corpus_sha256: str
    references_sha256: str
    reference_provenance: str
    split: Split
    context: bool
    numerical_threads: int = 1
    candidates: tuple[CandidateResult, ...]


def expand(result: Result, prepared: Prepared) -> Result:
    """Return source indices to the scorer, preserving duplicate identities."""
    return Result(
        tuple(result.partition[unit] for unit in prepared.source_to_unit),
        tuple(
            Group(
                group.id,
                tuple(
                    index
                    for index, unit in enumerate(prepared.source_to_unit)
                    if unit in group.primary_members
                ),
                prepared.unit_to_source[group.representative],
            )
            for group in result.groups
        ),
        tuple(result.memberships[unit] for unit in prepared.source_to_unit),
    )


def primary_result(result: Result) -> Result:
    retained = {group.id for group in result.groups}
    return Result(
        result.partition,
        result.groups,
        tuple(
            (Membership(label, 1.0),) if label in retained else ()
            for label in result.partition
        ),
    )


def baseline(prepared: Prepared, window: int | None) -> Result:
    n = len(prepared.vectors)
    labels = tuple(index // window if window else 0 for index in range(n))
    retained = {
        label
        for label in set(labels)
        if sum(bool(prepared.substantive[i]) for i in range(n) if labels[i] == label)
        >= 2
    }
    groups = tuple(
        Group(
            label, tuple(i for i in range(n) if labels[i] == label), labels.index(label)
        )
        for label in sorted(retained)
    )
    return Result(
        labels,
        groups,
        tuple(
            (Membership(label, 1.0),) if label in retained else () for label in labels
        ),
    )


def thread_score(
    result: Result,
    reference: ThreadReference,
    prepared: Prepared,
    thread_hash: str,
    provider: str,
    length: str,
    seconds: float,
) -> ThreadResult:
    substantive = [
        i for i in reference.reviewed_indices if reference.kind(i) == Kind.SUBSTANTIVE
    ]
    return ThreadResult(
        thread_hash=thread_hash,
        provider=provider,
        length=length,
        source_messages=len(result.memberships),
        units=len(prepared.vectors),
        groups=len(result.groups),
        unreviewed_assumed_substantive=prepared.unreviewed_assumed_substantive,
        reviewed_substantive=len(substantive),
        assigned_substantive=sum(bool(result.memberships[i]) for i in substantive),
        seconds=seconds,
        partition=score(evaluate_pairs(primary_result(result), reference.pairs())),
        overlapping=score(evaluate_pairs(result, reference.pairs())),
        cases={
            case: score(evaluate_pairs(result, reference.pairs(case))) for case in Case
        },
    )


@click.command()
@click.option("--corpus", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--references", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--split", type=click.Choice([split.value for split in Split]), required=True
)
@click.option(
    "--context", is_flag=True, help="Second-round fixed 0.7/0.3 context view."
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--settings", type=click.Path(path_type=Path, exists=True))
def main(
    corpus: Path,
    references: Path,
    split: str,
    context: bool,
    output: Path,
    settings: Path | None,
) -> None:
    """Run only the requested split; never print histories or labels."""
    if not output.resolve().is_relative_to(
        Path("/home/james/scratch/mash-artifacts").resolve()
    ):
        raise click.ClickException("output must be in the private artifact directory")
    try:
        frozen, judgments, reference_hash = load_references(references, corpus)
    except (OSError, ValueError) as error:
        # Pydantic errors may contain source text. Expose only the error category.
        raise click.ClickException(
            f"cannot validate frozen inputs ({type(error).__name__})"
        ) from error
    configurations = CONFIGURATIONS
    if split == Split.HELD_OUT and settings is None:
        raise click.ClickException(
            "held-out scoring requires frozen development settings"
        )
    if settings is not None:
        selected = FrozenSettings.model_validate_json(settings.read_bytes())
        selected.verify(judgments.corpus_sha256, reference_hash)
        if any(selection.context != context for selection in selected.selections):
            raise click.ClickException("context flag differs from frozen settings")
        configurations = tuple(
            selection.configuration for selection in selected.selections
        )
    by_index = {reference.index: reference for reference in judgments.threads}
    reports: dict[str, list[ThreadResult]] = {}
    with threadpool_limits(limits=1):
        for index, entry in enumerate(frozen.selected):
            if entry.split.value != split:
                continue
            reference = by_index[index]
            prepared = prepare(entry.thread, reference, context)
            for setting in configurations:
                key = orjson.dumps(asdict(setting)).decode()
                start = time.perf_counter()
                result = cluster(prepared.vectors, setting, prepared.substantive)
                seconds = time.perf_counter() - start
                reports.setdefault(key, []).append(
                    thread_score(
                        expand(result, prepared),
                        reference,
                        prepared,
                        hashlib.sha256(entry.thread.id.encode()).hexdigest(),
                        entry.thread.provider.value,
                        entry.thread.length.value,
                        seconds,
                    )
                )
            # Baselines depend on source order, not the embedding view. Keep the
            # raw-projection dedup boundaries fixed in the context comparison.
            baseline_prepared = prepare(entry.thread, reference, context=False)
            for name, window in (
                ("whole_thread", None),
                ("singletons", 1),
                ("window_8", 8),
            ):
                result = expand(baseline(baseline_prepared, window), baseline_prepared)
                reports.setdefault(name, []).append(
                    thread_score(
                        result,
                        reference,
                        baseline_prepared,
                        hashlib.sha256(entry.thread.id.encode()).hexdigest(),
                        entry.thread.provider.value,
                        entry.thread.length.value,
                        0,
                    )
                )
            click.echo(f"Scored thread {index}; {len(prepared.vectors)} unique units")
    report = Report(
        corpus_sha256=judgments.corpus_sha256,
        references_sha256=reference_hash,
        reference_provenance=judgments.provenance,
        split=Split(split),
        context=context,
        candidates=tuple(
            CandidateResult(configuration=key, threads=tuple(values))
            for key, values in reports.items()
        ),
    )
    encoded = orjson.dumps(report.model_dump(mode="json"))
    with os.fdopen(
        os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb"
    ) as file:
        file.write(encoded)
    for candidate in report.candidates:
        click.echo(
            f"{candidate.configuration}: macro F1="
            f"{np.mean([thread.overlapping.f1 for thread in candidate.threads]):.3f}"
        )


if __name__ == "__main__":
    main()
