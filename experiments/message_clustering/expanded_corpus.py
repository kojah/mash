"""Fresh balanced cohort excluding every component from the first study."""

import hashlib
import os
import sqlite3
from pathlib import Path

import click
import orjson

from corpus import (
    Corpus,
    Length,
    Provider,
    SelectedThread,
    Split,
    Thread,
    digest,
    leakage_components,
    read_threads,
)

EXPANDED_SEED = "mash-message-clustering-alternatives-20261005-v1"
STRATUM_COUNTS = {
    (Provider.CLAUDE, Length.SHORT): 18,
    (Provider.CLAUDE, Length.MEDIUM): 6,
    (Provider.CLAUDE, Length.LONG): 6,
    (Provider.CODEX, Length.SHORT): 18,
    (Provider.CODEX, Length.MEDIUM): 12,
    (Provider.CODEX, Length.LONG): 12,
}


def select_expanded(
    threads: tuple[Thread, ...],
    components: tuple[tuple[str, ...], ...],
    previous: Corpus,
) -> tuple[SelectedThread, ...]:
    """Fixed available-data strata, two-thirds dev and one-third reserved."""
    excluded_ids = {entry.thread.id for entry in previous.selected}
    excluded_hashes = {entry.component for entry in previous.selected}
    component_of = {
        identity: component for component in components for identity in component
    }
    excluded = {
        component
        for component in components
        if excluded_ids.intersection(component)
        or digest("\n".join(component)) in excluded_hashes
    }
    used = set(excluded)
    selected: list[SelectedThread] = []
    for provider in Provider:
        for length in Length:
            candidates = sorted(
                (
                    thread
                    for thread in threads
                    if thread.provider == provider
                    and thread.length == length
                    and len(thread.messages) >= 8
                ),
                key=lambda thread: (
                    -int(len(thread.messages) > 1000),
                    digest(EXPANDED_SEED + thread.id),
                ),
            )
            count = 0
            for thread in candidates:
                component = component_of[thread.id]
                if component in used:
                    continue
                used.add(component)
                selected.append(
                    SelectedThread(
                        split=Split.HELD_OUT if count % 3 == 0 else Split.DEVELOPMENT,
                        component=digest("\n".join(component)),
                        thread=thread,
                    )
                )
                count += 1
                if count == STRATUM_COUNTS[provider, length]:
                    break
            if count != STRATUM_COUNTS[provider, length]:
                raise ValueError("insufficient fresh independent components in stratum")
    return tuple(selected)


@click.command()
@click.option("--database", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--previous", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(database: Path, previous: Path, output: Path) -> None:
    if not output.resolve().is_relative_to(Path("/home/james/scratch/mash-artifacts")):
        raise click.ClickException("output must be in the private artifact directory")
    try:
        old = Corpus.model_validate_json(previous.read_bytes())
        manifest, threads = read_threads(database)
        components, checked = leakage_components(threads)
        corpus = Corpus(
            selection_seed=EXPANDED_SEED,
            gold_manifest=manifest,
            source_threads=len(threads),
            ancestry_files_checked=checked,
            selected=select_expanded(threads, components, old),
        )
    except (OSError, sqlite3.Error, ValueError) as error:
        # Validation details can contain private source records.
        raise click.ClickException(
            f"corpus freeze failed ({type(error).__name__}); no corpus written"
        ) from error
    encoded = orjson.dumps(corpus.model_dump(mode="json"))
    with os.fdopen(
        os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb"
    ) as file:
        file.write(encoded)
    click.echo(
        f"Frozen {len(corpus.selected)} threads; "
        f"SHA-256 {hashlib.sha256(encoded).hexdigest()}"
    )


if __name__ == "__main__":
    main()
