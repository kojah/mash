"""Report source coverage, registered strata and measured follow-up costs."""

import hashlib
import os
import re
import sqlite3
from collections import defaultdict
from pathlib import Path

import click
import numpy as np
import orjson

from build_input_followup import Built
from corpus import Record, Role, Thread
from followup_inputs import Input, Variant, ancestor
from run_input_followup import FollowupReport
from study_analysis import FitCost, Slice, build_costs, fit_costs, slices
from study_annotations import load_development
from study_artifacts import Cost, View
from summarize_turn_study import verify_rows
from turn_artifact import Artifact
from turn_study_corpus import Cohort, Phase
from turns import Span, Turn


def validate_sources(built: Built, thread: Thread, turns: tuple[Turn, ...]) -> None:
    validate_input_sources(built.inputs, built.turn_ids, thread, turns)


def validate_input_sources(
    inputs: tuple[Input, ...],
    owners: tuple[str, ...],
    thread: Thread,
    turns: tuple[Turn, ...],
) -> None:
    """Verify literal mappings and ancestry without inference or annotation access."""
    indices = {turn.id: index for index, turn in enumerate(turns)}
    for item in inputs:
        if item.turn_id not in indices or item.turn_id not in owners:
            raise ValueError("input has no owning prompted turn")
        current = indices[item.turn_id]
        turn = turns[current]
        if item.role not in (Role.USER, Role.ASSISTANT):
            raise ValueError("input has an unsupported role")
        allowed = set(turn.prompts if item.role == Role.USER else turn.answers)
        if item.context_turn_id is not None:
            previous = ancestor(thread, turns, current)
            if (
                item.role != Role.USER
                or previous is None
                or previous.turn_id != item.context_turn_id
            ):
                raise ValueError("input borrows from an impermissible ancestor")
            allowed.update(previous.sources)
        for location in item.content.locations:
            source = location.source
            if source.source not in allowed:
                raise ValueError("input contains a future or unrelated source")
            message = thread.messages[source.source]
            if (
                message.id != source.message_id
                or not 0 <= source.start <= source.end <= len(message.projection)
                or not 0 <= location.start <= location.end <= len(item.content.text)
                or item.content.text[location.start : location.end]
                != message.projection[source.start : source.end]
            ):
                raise ValueError("input text does not match its original source range")


def union_length(intervals: tuple[tuple[int, int], ...]) -> int:
    """Count source characters once despite overlapping token windows."""
    total = 0
    end = 0
    for left, right in sorted(intervals):
        if left < 0 or right < left:
            raise ValueError("invalid source interval")
        total += max(0, right - max(left, end))
        end = max(end, right)
    return total


class Coverage(Record):
    own_source_characters: int
    supplied_own_characters: int
    supplied_context_characters: int
    sources: int
    entirely_excluded_sources: int
    partially_supplied_sources: int
    context_turns: int


def coverage(
    turns: tuple[Turn, ...],
    lengths: tuple[int, ...],
    spans: dict[str, tuple[Span, ...]],
    context_turns: frozenset[str],
) -> Coverage:
    own = context = excluded = partial = sources = original = 0
    for turn in turns:
        by_source: dict[int, list[tuple[int, int]]] = defaultdict(list)
        for span in spans[turn.id]:
            if not 0 <= span.source < len(lengths) or span.end > lengths[span.source]:
                raise ValueError("span exceeds original source")
            by_source[span.source].append((span.start, span.end))
        counts = {
            index: union_length(tuple(intervals))
            for index, intervals in by_source.items()
        }
        for index in turn.sources:
            original += lengths[index]
            sources += 1
            represented = counts.get(index, 0)
            own += represented
            excluded += int(lengths[index] > 0 and represented == 0)
            partial += int(0 < represented < lengths[index])
        context += sum(
            count for index, count in counts.items() if index not in turn.sources
        )
    return Coverage(
        own_source_characters=original,
        supplied_own_characters=own,
        supplied_context_characters=context,
        sources=sources,
        entirely_excluded_sources=excluded,
        partially_supplied_sources=partial,
        context_turns=len(context_turns),
    )


class Construction(Record):
    variant: Variant
    histories: int
    p50_seconds: float
    p95_seconds: float
    new_vectors: int
    input_tokens: int
    coverage: Coverage
    timing_description: str


class Resources(Record):
    build_peak_kib: int
    score_peak_kib: int
    memory_gate_passed: bool


def peak_rss(path: Path) -> int:
    text = path.read_text()
    if not re.search(r"^\s*Exit status: 0\s*$", text, re.MULTILINE):
        raise ValueError("resource log is incomplete or command failed")
    match = re.search(
        r"^\s*Maximum resident set size \(kbytes\): (\d+)\s*$", text, re.MULTILINE
    )
    if not match:
        raise ValueError("resource log has no measured peak RSS")
    return int(match.group(1))


class Analysis(Record):
    development_sha256: str
    scores: tuple[Slice, ...]
    clustering: tuple[FitCost, ...]
    construction: tuple[Construction, ...]
    resources: Resources
    artifact_construction_gate_passed: bool
    clustering_gate_passed: bool
    source_coverage_description: str = (
        "Unicode characters supplied to attention, unioned per source and turn; "
        "includes whitespace and does not imply semantic sufficiency"
    )


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--development", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--original", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--build-time", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--score-time", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    development: Path,
    annotations: Path,
    artifacts: Path,
    original: Path,
    build_time: Path,
    score_time: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    raw = development.read_bytes()
    followup = FollowupReport.model_validate_json(raw)
    report = followup.report
    data = cohort.read_bytes()
    if hashlib.sha256(data).hexdigest() != report.cohort_sha256:
        raise click.ClickException("cohort changed after scoring")
    captured = Cohort.model_validate_json(data)
    labels = load_development(annotations)
    if (
        labels.manifest_sha256 != report.annotation_manifest_sha256
        or labels.file_sha256 != report.annotation_sha256
    ):
        raise click.ClickException("annotations changed after scoring")
    indices = frozenset(
        entry.captured.index
        for entry in captured.entries
        if entry.phase == Phase.DEVELOPMENT
    )
    names = frozenset(
        summary.name for summary in (*report.candidates, *report.baselines)
    )
    verify_rows(report.rows, indices, names)
    refs = {ref.pair_id: ref for ref in labels.manifest.references}
    old_costs = tuple(
        Cost.model_validate_json(line)
        for line in (original / "costs.jsonl").read_bytes().splitlines()
        if line
    )
    build_costs(
        old_costs, frozenset(entry.captured.index for entry in captured.entries)
    )
    source_store = sqlite3.connect(
        (original / "gold.sqlite").resolve().as_uri() + "?mode=ro", uri=True
    )
    observations: dict[Variant, list[tuple[float, int, int, Coverage]]] = defaultdict(
        list
    )
    try:
        for entry in captured.entries:
            thread = entry.captured
            turns = tuple(turn for turn in thread.turns if turn.prompts)
            lengths = tuple(
                len(message.projection) for message in thread.thread.messages
            )
            for variant in Variant:
                name = f"{thread.index}-{variant}"
                provenance = (artifacts / f"{name}.json").read_bytes()
                if (
                    entry.phase == Phase.DEVELOPMENT
                    and hashlib.sha256(provenance).hexdigest()
                    != report.artifact_sha256[f"{name}.json"]
                ):
                    raise click.ClickException("scored provenance changed")
                built = Built.model_validate_json(provenance)
                validate_sources(built, thread.thread, thread.turns)
                spans = {
                    turn.id: tuple(
                        location.source
                        for item in built.inputs
                        if item.turn_id == turn.id
                        for location in item.content.locations
                    )
                    for turn in turns
                }
                seconds, tokens, new = (
                    built.seconds,
                    built.input_tokens,
                    built.new_vectors,
                )
                contexts = frozenset(
                    item.turn_id for item in built.inputs if item.context_turn_id
                )
                if variant == Variant.RAW:
                    row = source_store.execute(
                        "SELECT manifest FROM artifacts WHERE name=?",
                        (f"{thread.index}-{View.SEPARATE}",),
                    ).fetchone()
                    if not row:
                        raise click.ClickException("original raw manifest is absent")
                    artifact = Artifact.model_validate_json(row[0])
                    if (
                        artifact.source_sha256 != report.cohort_sha256
                        or artifact.protocol_sha256 != labels.manifest.protocol_sha256
                        or artifact.turns != turns
                    ):
                        raise click.ClickException("original source manifest differs")
                    spans = {
                        turn.id: tuple(
                            span
                            for unit in artifact.units
                            if unit.turn_id == turn.id
                            for span in unit.spans
                        )
                        for turn in turns
                    }
                    cost = next(
                        cost
                        for cost in old_costs
                        if cost.thread_index == thread.index
                        and cost.view == View.SEPARATE
                    )
                    seconds, tokens, new = (
                        cost.seconds,
                        cost.input_tokens,
                        cost.new_vectors,
                    )
                observations[variant].append(
                    (seconds, new, tokens, coverage(turns, lengths, spans, contexts))
                )
    finally:
        source_store.close()
    construction = tuple(
        Construction(
            variant=variant,
            histories=len(values),
            p50_seconds=float(np.quantile([value[0] for value in values], 0.50)),
            p95_seconds=float(np.quantile([value[0] for value in values], 0.95)),
            new_vectors=sum(value[1] for value in values),
            input_tokens=sum(value[2] for value in values),
            coverage=Coverage(
                **{
                    field: sum(getattr(value[3], field) for value in values)
                    for field in Coverage.model_fields
                }
            ),
            timing_description="original cached build; control-copy time excluded"
            if variant == Variant.RAW
            else "cache-assisted follow-up construction",
        )
        for variant, values in sorted(observations.items())
    )
    fitting = fit_costs(report.rows)
    build_peak, score_peak = peak_rss(build_time), peak_rss(score_time)
    result = Analysis(
        development_sha256=hashlib.sha256(raw).hexdigest(),
        scores=slices(report.rows, labels.labels, refs),
        clustering=fitting,
        construction=construction,
        resources=Resources(
            build_peak_kib=build_peak,
            score_peak_kib=score_peak,
            memory_gate_passed=max(build_peak, score_peak) <= 4 * 1024**2,
        ),
        artifact_construction_gate_passed=all(
            cost.p95_seconds <= 300 for cost in construction
        ),
        clustering_gate_passed=all(cost.p95_seconds <= 10 for cost in fitting),
    )
    with output.open("xb") as destination:
        destination.write(orjson.dumps(result.model_dump(mode="json")))
    click.echo(
        "Wrote source coverage and breakdowns; no new fits or confirmation access"
    )


if __name__ == "__main__":
    main()
