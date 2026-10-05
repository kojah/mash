"""Freeze a balanced bundle audit and structural checks before another sweep."""

import gzip
import hashlib
from collections import Counter
from enum import StrEnum
from pathlib import Path

import click
import orjson

from corpus import Corpus, Length, Provider, Record, Split, Thread
from turn_ancestry import Capture, CapturedThread, bridge, native_check
from turns import Turn, extract, injected

SEED = "mash-turn-ancestry-audit-20261005-v1"


class Case(StrEnum):
    PAIRED = "paired_bundle"
    ORPHAN = "orphan"
    INTERRUPTION = "hidden_response_interruption"


class Sample(Record):
    id: str
    thread_index: int
    kind: Case
    turn: Turn
    preceding_sources: tuple[int, ...]
    following_source: int | None


class Check(Record):
    index: int
    canonical: int
    source_messages: int
    eligible_messages: int
    old_turns: int
    bridged_turns: int
    turns: int
    old_orphans: int
    orphans: int
    native_matched: int
    native_checked: int
    native_mismatches: int
    bridged_visible_parents: int
    skipped_records: int


class Audit(Record):
    capture_sha256: str
    selection_seed: str = SEED
    checks: tuple[Check, ...]
    samples: tuple[Sample, ...]


def original_thread(item: CapturedThread) -> Thread:
    return item.thread.model_copy(
        update={
            "messages": tuple(
                message.model_copy(update={"context_parent": resolution.old_parent})
                for message, resolution in zip(
                    item.thread.messages, item.resolutions, strict=True
                )
            )
        }
    )


def load_capture(
    path: Path, corpus: Corpus, corpus_hash: str
) -> dict[int, CapturedThread]:
    """Require the exact development cohort and recheck frozen native evidence."""
    capture = Capture.model_validate_json(path.read_bytes())
    if (
        capture.original_corpus_sha256 != corpus_hash
        or capture.split != Split.DEVELOPMENT
    ):
        raise ValueError("ancestry capture differs from development corpus")
    expected = {
        i: entry.thread
        for i, entry in enumerate(corpus.selected)
        if entry.split == Split.DEVELOPMENT
    }
    if len(capture.threads) != len(expected) or {
        t.index for t in capture.threads
    } != set(expected):
        raise ValueError("ancestry capture must cover development threads exactly once")
    result = {}
    for item in capture.threads:
        if original_thread(item) != expected[item.index]:
            raise ValueError("ancestry capture changed frozen source messages")
        raw = gzip.decompress(
            (path.parent / f"source-{item.index}.jsonl.gz").read_bytes()
        )
        result[item.index] = checked(item, raw)[0]
    return result


def checked(item: CapturedThread, raw: bytes) -> tuple[CapturedThread, Check]:
    """Validate complete inputs and coverage; do not score semantic relationships."""
    if hashlib.sha256(raw).hexdigest() != item.source_sha256:
        raise ValueError("captured provider source changed")
    fixed, resolutions, activities = bridge(original_thread(item), item.canonical)
    if resolutions != item.resolutions or activities != item.activities:
        raise ValueError("ancestry reconstruction is not reproducible")
    turns = extract(fixed, activities)
    # Branch reason repair does not alter source membership; compare identities.
    if tuple(turn.sources for turn in turns) != tuple(
        turn.sources for turn in item.turns
    ):
        raise ValueError("captured turn membership changed")
    expected = {
        i
        for i, m in enumerate(fixed.messages)
        if m.projection.strip() and not injected(m)
    }
    observed = Counter(i for turn in turns for i in turn.sources)
    if set(observed) != expected or any(count != 1 for count in observed.values()):
        raise ValueError("turn extraction lost or duplicated eligible sources")
    for turn in turns:
        seen = set()
        for i in turn.sources:
            message = fixed.messages[i]
            if (
                seen
                and message.context_parent is not None
                and message.context_parent not in seen
            ):
                raise ValueError("turn crossed an explicit branch")
            seen.add(message.ordinal)
        if turn.answers and turn.prompts and max(turn.prompts) > min(turn.answers):
            raise ValueError("prompt follows an answer inside one turn")
    matched, native_parents, mismatches, _ = native_check(fixed, item.canonical, raw)
    if mismatches:
        raise ValueError("native and canonical ancestry disagree")
    if fixed.provider == Provider.CLAUDE and (
        matched != len(item.canonical) or native_parents != len(item.canonical)
    ):
        raise ValueError("incomplete native Claude ancestry coverage")
    result = item.model_copy(update={"thread": fixed, "turns": turns})
    return result, Check(
        index=item.index,
        canonical=len(item.canonical),
        source_messages=len(fixed.messages),
        eligible_messages=len(expected),
        old_turns=len(item.original_turns),
        bridged_turns=len(item.bridged_turns),
        turns=len(turns),
        old_orphans=sum(not t.prompts for t in item.original_turns),
        orphans=sum(not t.prompts for t in turns),
        native_matched=matched,
        native_checked=native_parents,
        native_mismatches=mismatches,
        bridged_visible_parents=sum(r.old_parent != r.parent for r in resolutions),
        skipped_records=sum(len(r.skipped) for r in resolutions),
    )


def samples(threads: tuple[CapturedThread, ...]) -> tuple[Sample, ...]:
    selected: list[tuple[CapturedThread, Turn, Case]] = []
    for provider in Provider:
        for length in Length:
            candidates = sorted(
                (
                    t
                    for t in threads
                    if t.thread.provider == provider and t.thread.length == length
                ),
                key=lambda t: hashlib.sha256((SEED + t.thread.id).encode()).hexdigest(),
            )
            count = 0
            for item in candidates:
                paired = [turn for turn in item.turns if turn.prompts and turn.answers]
                if not paired:
                    continue
                # Prefer the bundle exposing the largest ancestry repair.
                chosen = max(
                    paired,
                    key=lambda t: sum(
                        bool(item.resolutions[i].skipped) for i in t.sources
                    ),
                )
                selected.append((item, chosen, Case.PAIRED))
                count += 1
                if count == 2:
                    break
            if count != 2:
                raise ValueError("missing audit stratum")
    for item in threads:
        selected.extend(
            (item, turn, Case.ORPHAN) for turn in item.turns if not turn.prompts
        )
        for turn in item.turns:
            if turn.prompts and any(
                turn.prompts[0] in prior.prompts[1:] for prior in item.bridged_turns
            ):
                selected.append((item, turn, Case.INTERRUPTION))
    result = []
    for item, turn, kind in selected:
        first = min(turn.sources)
        last = max(turn.sources)
        preceding = tuple(
            i for i in range(first) if item.thread.messages[i].projection.strip()
        )[-2:]
        following = next(
            (
                i
                for i in range(last + 1, len(item.thread.messages))
                if not injected(item.thread.messages[i])
            ),
            None,
        )
        result.append(
            Sample(
                id=hashlib.sha256(
                    f"{item.index}:{turn.id}:{kind}".encode()
                ).hexdigest()[:16],
                thread_index=item.index,
                kind=kind,
                turn=turn,
                preceding_sources=preceding,
                following_source=following,
            )
        )
    return tuple(result)


@click.command()
@click.option("--capture", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(capture: Path, output: Path) -> None:
    if not output.resolve().is_relative_to(Path("/home/james/scratch/mash-artifacts")):
        raise click.ClickException("use private artifact root")
    encoded = capture.read_bytes()
    source = Capture.model_validate_json(encoded)
    threads = []
    checks = []
    for item in source.threads:
        raw = gzip.decompress(
            (capture.parent / f"source-{item.index}.jsonl.gz").read_bytes()
        )
        updated, check = checked(item, raw)
        threads.append(updated)
        checks.append(check)
    audit = Audit(
        capture_sha256=hashlib.sha256(encoded).hexdigest(),
        checks=tuple(checks),
        samples=samples(tuple(threads)),
    )
    with output.open("xb") as file:
        file.write(orjson.dumps(audit.model_dump(mode="json")))
    click.echo(
        f"Checked {len(checks)} development threads; {len(audit.samples)} manual cases"
    )
    click.echo(str(Counter(sample.kind for sample in audit.samples)))


if __name__ == "__main__":
    main()
