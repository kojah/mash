"""Build separate experimental gold, screen representations, retry fixed algorithms."""

import hashlib
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson

from alternatives import SETTINGS
from clustering import (
    CONFIGURATIONS,
    Configuration,
    Method,
    decode_stored_vector,
    normalized_vectors,
)
from corpus import Record, Split
from local_encoder import Encoder
from references import load_references
from turn_artifact import Artifact, Store, generation
from turn_evaluation import Fit, Inputs, bundling, isolated
from turns import Recipe, Unit, extract, joined, units, windows


class Candidate(Record):
    recipe: Recipe
    setting: str
    outcomes: tuple[Fit, ...]


class Build(Record):
    recipe: Recipe
    turns: int
    units: int
    new_vectors: int
    discarded_tokens: int
    seconds: float
    source_characters: int
    embedded_characters: int
    repeated_input_tokens: int


class Report(Record):
    corpus_sha256: str
    references_sha256: str
    protocol_sha256: str
    generation: str
    builds: tuple[Build, ...]
    bundling: tuple[Fit, ...]
    candidates: tuple[Candidate, ...]
    selected_recipe: Recipe
    total_seconds: float
    controls: dict[str, tuple[Fit, ...]]
    probe_cosines: tuple[float, ...]
    reuse_seconds: float
    artifact_bytes: int


@click.command()
@click.option("--corpus", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--references", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--snapshot", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output-dir", type=click.Path(path_type=Path), required=True)
def main(corpus: Path, references: Path, snapshot: Path, output_dir: Path) -> None:
    if not output_dir.resolve().is_relative_to(
        Path("/home/james/scratch/mash-artifacts")
    ):
        raise click.ClickException("use private artifact root")
    output_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    report_path = output_dir / "report.json"
    if report_path.exists():
        raise click.ClickException("refusing to overwrite completed experiment")
    os.umask(0o077)
    start = time.perf_counter()
    source, judgments, reference_hash = load_references(references, corpus)
    protocol = Path(__file__).resolve().parents[2] / "docs/turn-embedding-experiment.md"
    protocol_hash = hashlib.sha256(protocol.read_bytes()).hexdigest()
    encoder = Encoder(snapshot)
    metadata = output_dir / "encoder.json"
    encoded_manifest = orjson.dumps(encoder.manifest.model_dump())
    if metadata.exists():
        if metadata.read_bytes() != encoded_manifest:
            raise click.ClickException("resume encoder differs from recorded assets")
    else:
        with metadata.open("xb") as file:
            file.write(encoded_manifest)
    store = Store(output_dir / "gold.sqlite")
    by_index = {reference.index: reference for reference in judgments.threads}
    inputs: dict[Recipe, list[Inputs]] = {recipe: [] for recipe in Recipe}
    builds: list[Build] = []
    controls: dict[str, tuple[Fit, ...]] = {}
    probe_cosines: list[float] = []
    try:
        for recipe in Recipe:
            began = time.perf_counter()
            new_vectors = dropped = source_characters = embedded_characters = 0
            all_turns = []
            all_units = []
            for index, selected in enumerate(source.selected):
                if selected.split != Split.DEVELOPMENT:
                    continue
                thread = selected.thread
                turns = extract(thread)
                prepared = tuple(
                    unit
                    for turn in turns
                    for unit in units(thread, turn, recipe, encoder.tokenizer)
                )
                if recipe == Recipe.POOLED:
                    prepared = tuple(
                        Unit(
                            id=turn.id,
                            turn_id=turn.id,
                            text="",
                            spans=(),
                            represented_sources=turn.sources,
                            tokens=0,
                        )
                        for turn in turns
                    )
                    vectors = (
                        normalized_vectors(
                            np.stack(
                                [
                                    np.mean(
                                        [
                                            decode_stored_vector(
                                                bytes.fromhex(
                                                    thread.messages[i].vector_hex
                                                ),
                                                thread.messages[i].dimensions,
                                                thread.messages[i].norm,
                                            )
                                            for i in turn.sources
                                        ],
                                        axis=0,
                                    )
                                    for turn in turns
                                ]
                            )
                        )
                        if turns
                        else np.empty((0, 384))
                    )
                else:

                    def progress(
                        done: int,
                        total: int,
                        recipe: Recipe = recipe,
                        index: int = index,
                    ) -> None:
                        if done % 128 == 0 or done == total:
                            click.echo(
                                f"{recipe}, thread {index}: inferred {done}/{total}"
                            )

                    vectors, count = store.vectors(
                        tuple(unit.text for unit in prepared), encoder, progress
                    )
                    new_vectors += count
                dropped += sum(unit.discarded_tokens for unit in prepared)
                source_characters += sum(
                    len(thread.messages[i].projection)
                    for turn in turns
                    for i in turn.sources
                )
                # Union spans counts coverage once despite overlapping windows.
                covered: dict[int, set[int]] = {}
                for unit in prepared:
                    for span in unit.spans:
                        covered.setdefault(span.source, set()).update(
                            range(span.start, span.end)
                        )
                embedded_characters += sum(len(chars) for chars in covered.values())
                all_turns.extend(turns)
                all_units.extend(prepared)
                inputs[recipe].append(
                    Inputs(thread, turns, prepared, vectors, by_index[index])
                )
            store.publish(
                recipe.value,
                Artifact(
                    source_sha256=judgments.corpus_sha256,
                    protocol_sha256=protocol_hash,
                    encoder_generation=generation(encoder),
                    turns=tuple(all_turns),
                    units=tuple(all_units),
                ),
            )
            builds.append(
                Build(
                    recipe=recipe,
                    turns=len(all_turns),
                    units=len(all_units),
                    new_vectors=new_vectors,
                    discarded_tokens=dropped,
                    seconds=time.perf_counter() - began,
                    source_characters=source_characters,
                    embedded_characters=embedded_characters,
                    repeated_input_tokens=sum(unit.tokens for unit in all_units),
                )
            )
            click.echo(
                f"Built {recipe}: {len(all_units)} units in {builds[-1].seconds:.1f}s"
            )
        raw_inputs: list[Inputs] = []
        stored_inputs: list[Inputs] = []
        for selected, reference in (
            (entry, by_index[index])
            for index, entry in enumerate(source.selected)
            if entry.split == Split.DEVELOPMENT
        ):
            thread = selected.thread
            turns = extract(thread)
            owners = {i: turn.id for turn in turns for i in turn.sources}
            raw_units = []
            stored_vectors = []
            for i in sorted(owners):
                text, spans = joined(thread, (i,))
                bounded = windows(encoder.tokenizer, text, spans, 510, first_only=True)
                if not bounded:
                    continue
                raw_units.append(
                    Unit(
                        id=thread.messages[i].id,
                        turn_id=owners[i],
                        text=bounded[0][0],
                        spans=bounded[0][1],
                        represented_sources=(i,),
                        tokens=0,
                    )
                )
                message = thread.messages[i]
                stored_vectors.append(
                    decode_stored_vector(
                        bytes.fromhex(message.vector_hex),
                        message.dimensions,
                        message.norm,
                    )
                )
            vectors, _ = store.vectors(
                tuple(unit.text for unit in raw_units),
                encoder,
                lambda done, total: None,
            )
            stored = np.stack(stored_vectors)
            if len(probe_cosines) < 64:
                probe_cosines.extend(
                    float(a @ b) for a, b in zip(vectors, stored, strict=True)
                )
                probe_cosines = probe_cosines[:64]
            raw_inputs.append(
                Inputs(thread, turns, tuple(raw_units), vectors, reference)
            )
            stored_inputs.append(
                Inputs(thread, turns, tuple(raw_units), stored, reference)
            )
            click.echo(f"Raw-message control: {len(raw_inputs)}/48 threads")
        for name, messages in (
            ("same_runtime_message", raw_inputs),
            ("stored_message", stored_inputs),
        ):
            for cutoff in (0.35, 0.50, 0.65):
                controls[f"{name}/{cutoff}"] = tuple(
                    isolated(item, Configuration(Method.AVERAGE_LINK, cutoff))
                    for item in messages
                )
        candidates = []
        for recipe in Recipe:
            for cutoff in (0.35, 0.50, 0.65):
                setting = Configuration(Method.AVERAGE_LINK, cutoff)
                outcomes = tuple(isolated(item, setting) for item in inputs[recipe])
                candidates.append(
                    Candidate(recipe=recipe, setting=str(setting), outcomes=outcomes)
                )
                click.echo(
                    f"Screen {recipe}/{cutoff}: cross-turn F1 {macro(outcomes):.3f}"
                )
        # Development selection only, ties follow recipe and cutoff registration order.
        winner = max(candidates, key=lambda candidate: macro(candidate.outcomes)).recipe
        click.echo(f"Retry fixed algorithms on {winner}")
        for setting in (*CONFIGURATIONS, *SETTINGS):
            outcomes = tuple(isolated(item, setting) for item in inputs[winner])
            candidates.append(
                Candidate(recipe=winner, setting=str(setting), outcomes=outcomes)
            )
            click.echo(f"{setting}: cross-turn macro F1 {macro(outcomes):.3f}")
        reuse_start = time.perf_counter()
        for recipe, entries in inputs.items():
            if recipe == Recipe.POOLED:
                continue
            for item in entries:
                reloaded, count = store.vectors(
                    tuple(unit.text for unit in item.units),
                    encoder,
                    lambda done, total: None,
                )
                if count or not np.array_equal(reloaded, item.vectors):
                    raise ValueError("artifact reuse mismatch")
        report = Report(
            corpus_sha256=judgments.corpus_sha256,
            references_sha256=reference_hash,
            protocol_sha256=protocol_hash,
            generation=generation(encoder),
            builds=tuple(builds),
            bundling=tuple(bundling(item) for item in inputs[winner]),
            candidates=tuple(candidates),
            selected_recipe=winner,
            total_seconds=time.perf_counter() - start,
            controls=controls,
            probe_cosines=tuple(probe_cosines),
            reuse_seconds=time.perf_counter() - reuse_start,
            artifact_bytes=(output_dir / "gold.sqlite").stat().st_size,
        )
        with report_path.open("xb") as file:
            file.write(orjson.dumps(report.model_dump(mode="json")))
        click.echo(f"Completed in {report.total_seconds:.1f}s")
    finally:
        store.close()


def macro(outcomes: tuple[Fit, ...]) -> float:
    if any(outcome.status != "complete" for outcome in outcomes):
        return -1
    scores = [
        outcome.cross_turn.f1
        for outcome in outcomes
        if outcome.cross_turn and outcome.cross_turn.tp + outcome.cross_turn.fn
    ]
    return float(np.mean(scores)) if scores else 0


if __name__ == "__main__":
    main()
