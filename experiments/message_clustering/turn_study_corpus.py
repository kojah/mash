"""Freeze a broader multi-turn cohort without opening old reserved references."""

import hashlib
import os
import sqlite3
from collections import Counter
from contextlib import ExitStack
from enum import StrEnum
from pathlib import Path

import click
import orjson

from corpus import (
    Corpus,
    Provider,
    Record,
    Split,
    Thread,
    digest,
    leakage_components,
    read_threads,
)
from snapshot_node import Snapshot
from turn_ancestry import (
    CapturedThread,
    NativeAncestryError,
    bridge,
    canonical_history,
    capture_thread,
    verify_native,
)
from turns import extract

SEED = "mash-turn-study-20261005-v1"


class Phase(StrEnum):
    DEVELOPMENT = "development"
    CONFIRMATION = "confirmation"


class Entry(Record):
    phase: Phase
    component: str
    previously_used: bool
    node: str = "xenia"
    captured: CapturedThread


class Cohort(Record):
    seed: str = SEED
    previous_sha256: tuple[str, ...]
    gold_manifest_sha256: str
    histories_considered: int
    native_lineage_files_checked: int
    rejected: dict[str, int]
    target_threads: int = 48
    minimum_prompted_turns: int = 8
    snapshot_sha256: tuple[str, ...] = ()
    confirmation_per_provider: int = 6
    entries: tuple[Entry, ...]


def choose(
    candidates: tuple[Thread, ...],
    components: dict[str, tuple[str, ...]],
    reserved: set[str],
    used_ids: set[str],
    target: int = 48,
    minimum: int = 32,
    confirmation_per_provider: int = 6,
    long_ids: frozenset[str] = frozenset(),
) -> tuple[tuple[Thread, Phase, str], ...]:
    """Favor longer histories; reserve fresh components before development."""
    if target <= 2 * confirmation_per_provider or minimum > target:
        raise ValueError("confirmation allocation must leave development slots")
    available = sorted(
        candidates, key=lambda t: (t.id not in long_ids, digest(SEED + t.id))
    )
    seen = {components[i] for i in reserved if i in components}
    selected: list[tuple[Thread, Phase, str]] = []
    for provider in Provider:
        count = 0
        for thread in available:
            component = components[thread.id]
            if (
                thread.provider != provider
                or component in seen
                or used_ids.intersection(component)
            ):
                continue
            seen.add(component)
            selected.append((thread, Phase.CONFIRMATION, digest("\n".join(component))))
            count += 1
            if count == confirmation_per_provider:
                break
    if len(selected) < 4:
        raise ValueError("need at least four fresh confirmation components")
    # Round-robin providers rather than letting the larger provider consume all slots.
    pools = {p: [t for t in available if t.provider == p] for p in Provider}
    for position in range(max(len(pool) for pool in pools.values())):
        for provider in Provider:
            if position >= len(pools[provider]):
                continue
            thread = pools[provider][position]
            component = components[thread.id]
            if component in seen:
                continue
            seen.add(component)
            selected.append((thread, Phase.DEVELOPMENT, digest("\n".join(component))))
            if len(selected) == target:
                return tuple(selected)
    if len(selected) < minimum:
        raise ValueError(
            f"need at least {minimum} independent multi-turn histories; "
            f"available: {len(selected)}"
        )
    return tuple(selected)


@click.command()
@click.option("--database", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--previous",
    type=click.Path(path_type=Path, exists=True),
    multiple=True,
    required=True,
)
@click.option("--output-dir", type=click.Path(path_type=Path), required=True)
@click.option("--snapshot", type=click.Path(path_type=Path, exists=True), multiple=True)
@click.option("--confirmation-per-provider", type=click.IntRange(min=2), default=6)
@click.option("--threads", type=click.IntRange(min=32), default=48, show_default=True)
@click.option(
    "--min-prompted-turns",
    type=click.IntRange(min=2, max=250),
    default=8,
    show_default=True,
)
def main(
    database: Path,
    previous: tuple[Path, ...],
    output_dir: Path,
    threads: int,
    min_prompted_turns: int,
    snapshot: tuple[Path, ...],
    confirmation_per_provider: int,
) -> None:
    if not output_dir.resolve().is_relative_to(
        Path("/home/james/scratch/mash-artifacts")
    ):
        raise click.ClickException("use private artifact root")
    os.umask(0o077)
    old_bytes = tuple(path.read_bytes() for path in previous)
    old = tuple(Corpus.model_validate_json(data) for data in old_bytes)
    reserved_threads = tuple(
        e.thread for c in old for e in c.selected if e.split == Split.HELD_OUT
    )
    reserved = {t.id for t in reserved_threads}
    used_ids = {e.thread.id for c in old for e in c.selected}
    manifest, current = read_threads(database)
    manifests = [manifest]
    histories_by_id = {t.id: t for t in current}
    databases = {t.id: database for t in current}
    nodes = {t.id: "xenia" for t in current}
    snapshot_hashes = []
    for directory in snapshot:
        snapshot_bytes = (directory / "snapshot.json").read_bytes()
        captured = Snapshot.model_validate_json(snapshot_bytes)
        snapshot_hashes.append(hashlib.sha256(snapshot_bytes).hexdigest())
        copy_database = directory / "mash.sqlite3"
        copy_manifest, copy_threads = read_threads(copy_database)
        manifests.append(copy_manifest)
        sources = {s.conversation_id: s for s in captured.sources}
        for thread in copy_threads:
            source = sources.get(thread.id)
            if source is None:
                continue
            path = (directory / source.captured_path).resolve()
            if not path.is_relative_to(directory.resolve()):
                raise ValueError("snapshot source escapes capture directory")
            with path.open("rb") as file:
                if hashlib.file_digest(file, "sha256").hexdigest() != source.sha256:
                    raise ValueError("snapshot source checksum changed")
            if thread.id in histories_by_id:
                if (
                    thread.projection_hashes
                    != histories_by_id[thread.id].projection_hashes
                ):
                    raise ValueError("same conversation identity differs across nodes")
                continue
            histories_by_id[thread.id] = thread.model_copy(update={"source_path": path})
            databases[thread.id] = copy_database
            nodes[thread.id] = captured.node
    histories = tuple(histories_by_id.values())
    local = tuple(t for t in histories if t.source_path.is_file())
    # Old native sources remain protection-only evidence even after DB removal.
    guards = {
        e.thread.id: e.thread
        for c in old
        for e in c.selected
        if e.thread.id not in histories_by_id and e.thread.source_path.is_file()
    }
    lineage_pool = local + tuple(guards.values())
    lineage, checked = leakage_components(lineage_pool)
    components = {t.id: group for t, group in zip(lineage_pool, lineage, strict=True)}
    reserved_sessions = {(t.provider, t.session) for t in reserved_threads if t.session}
    used_sessions = {
        (e.thread.provider, e.thread.session)
        for c in old
        for e in c.selected
        if e.thread.session
    }
    for thread in local:
        if (thread.provider, thread.session) in reserved_sessions:
            reserved.add(thread.id)
        if (thread.provider, thread.session) in used_sessions:
            used_ids.add(thread.id)
    rejected: Counter[str] = Counter()
    candidates = []
    long_ids = set()
    canonical_by_id = {}
    reserved_hashes = tuple(t.projection_hashes for t in reserved_threads)
    with ExitStack() as stack:
        connections = {}
        for path in set(databases.values()):
            db = stack.enter_context(
                sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro", uri=True)
            )
            db.execute("BEGIN")
            connections[path] = db
        for thread in local:
            component = components[thread.id]
            if reserved.intersection(component):
                rejected["reserved_lineage"] += 1
                continue
            # Digest-only protection when a reserved source isn't in the current DB.
            hashes = thread.projection_hashes
            if any(
                len(hashes & h) >= 3
                and len(hashes & h) >= 0.5 * min(len(hashes), len(h))
                for h in reserved_hashes
            ):
                rejected["reserved_copy"] += 1
                continue
            canonical = canonical_history(connections[databases[thread.id]], thread.id)
            fixed, _, activities = bridge(thread, canonical)
            count = sum(bool(t.prompts) for t in extract(fixed, activities))
            if not min_prompted_turns <= count <= 250:
                rejected["turn_count_outside_range"] += 1
                continue
            try:
                verify_native(thread, canonical, thread.source_path.read_bytes())
            except NativeAncestryError as error:
                rejected[f"native: {error.reason.value}"] += 1
                continue
            candidates.append(thread)
            if count >= 8:
                long_ids.add(thread.id)
            canonical_by_id[thread.id] = canonical
        click.echo(f"Eligible histories: {Counter(t.provider for t in candidates)}")
        click.echo(
            f"Independent components: {len({components[t.id] for t in candidates})}"
        )
        selected = choose(
            tuple(candidates),
            components,
            reserved,
            used_ids,
            target=threads,
            minimum=threads,
            confirmation_per_provider=confirmation_per_provider,
            long_ids=frozenset(long_ids),
        )
        output_dir.mkdir(parents=True, mode=0o700, exist_ok=False)
        entries = tuple(
            Entry(
                phase=phase,
                component=component,
                previously_used=thread.id in used_ids,
                node=nodes[thread.id],
                captured=capture_thread(
                    thread, canonical_by_id[thread.id], index, output_dir
                ),
            )
            for index, (thread, phase, component) in enumerate(selected)
        )
    cohort = Cohort(
        previous_sha256=tuple(hashlib.sha256(b).hexdigest() for b in old_bytes),
        gold_manifest_sha256=hashlib.sha256("\0".join(manifests).encode()).hexdigest(),
        histories_considered=len(histories),
        native_lineage_files_checked=checked,
        rejected=dict(rejected),
        target_threads=threads,
        minimum_prompted_turns=min_prompted_turns,
        snapshot_sha256=tuple(snapshot_hashes),
        confirmation_per_provider=confirmation_per_provider,
        entries=entries,
    )
    with (output_dir / "cohort.json").open("xb") as file:
        file.write(orjson.dumps(cohort.model_dump(mode="json")))
    click.echo(f"Frozen {len(entries)} multi-turn histories")
    click.echo(str(Counter((e.phase, e.captured.thread.provider) for e in entries)))
    click.echo(f"New histories: {sum(not e.previously_used for e in entries)}")


if __name__ == "__main__":
    main()
