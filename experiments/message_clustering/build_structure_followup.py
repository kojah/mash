"""Build registered structural chunk vectors on installed assets, without labels."""

import hashlib
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson

from analyze_input_followup import validate_input_sources
from build_input_followup import Built, import_cache
from corpus import Record, Thread
from followup_inputs import Input, Variant, prepare
from local_encoder import Encoder
from structure_chunks import Mode, Prepared, prepare_structured
from turn_artifact import Store
from turn_study_corpus import Cohort, Phase
from turns import Turn


class StructuredArtifact(Record):
    thread_index: int
    phase: Phase
    mode: Mode
    turn_ids: tuple[str, ...]
    units: tuple[Prepared, ...]
    matrix_sha256: str
    source_input_sha256: str
    seconds: float
    new_vectors: int
    input_tokens: int


def own_mask(
    inputs: tuple[Input, ...], owner: str, source: int, length: int
) -> np.ndarray:
    result = np.zeros(length, dtype=np.bool_)
    for item in inputs:
        if item.turn_id != owner:
            continue
        for location in item.content.locations:
            if location.source.source == source:
                result[location.source.start : location.source.end] = True
    return result


def check_coverage(
    original: tuple[Input, ...],
    current: tuple[Input, ...],
    thread: Thread,
    turns: tuple[Turn, ...],
) -> None:
    """Require identical selected non-whitespace coverage, independent of packing."""
    for turn in turns:
        if not turn.prompts:
            continue
        for source in turn.sources:
            text = thread.messages[source].projection
            nonwhite = np.fromiter(
                (not char.isspace() for char in text), dtype=np.bool_
            )
            old = own_mask(original, turn.id, source, len(text))
            new = own_mask(current, turn.id, source, len(text))
            if np.any((old != new) & nonwhite):
                raise ValueError(
                    "structural selection changed non-whitespace source coverage"
                )


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--original", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path, protocol: Path, model: Path, original: Path, output: Path
) -> None:
    os.umask(0o077)
    data = cohort.read_bytes()
    cohort_sha = hashlib.sha256(data).hexdigest()
    if (original / "cohort.sha256").read_text().strip() != cohort_sha:
        raise ValueError("frozen source cohort changed")
    captured = Cohort.model_validate_json(data)
    encoder = Encoder(model)
    encoder_bytes = (original / "encoder.json").read_bytes()
    if encoder.manifest != type(encoder.manifest).model_validate_json(encoder_bytes):
        raise ValueError("frozen encoder changed")
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    (output / "encoder.json").write_bytes(encoder_bytes)
    (output / "cohort.sha256").write_text(cohort_sha)
    (output / "protocol.sha256").write_text(
        hashlib.sha256(protocol.read_bytes()).hexdigest()
    )
    store = Store(output / "vectors.sqlite")
    try:
        import_cache(store, original / "vectors.sqlite")
        for entry in captured.entries:
            thread = entry.captured
            turns = tuple(turn for turn in thread.turns if turn.prompts)
            owners = tuple(turn.id for turn in turns)
            source_path = original / f"{thread.index}-{Variant.CONTEXT_MEAN}.json"
            source_bytes = source_path.read_bytes()
            frozen = Built.model_validate_json(source_bytes)
            control = tuple(
                item
                for index, turn in enumerate(thread.turns)
                if turn.prompts
                for item in prepare(
                    thread.thread,
                    thread.turns,
                    index,
                    Variant.CONTEXT_MEAN,
                    encoder.tokenizer,
                )
            )
            if control != frozen.inputs or owners != frozen.turn_ids:
                raise ValueError("original control inputs no longer reproduce exactly")
            for mode in Mode:
                started = time.perf_counter()
                units = tuple(
                    unit
                    for index, turn in enumerate(thread.turns)
                    if turn.prompts
                    for unit in prepare_structured(
                        thread.thread, thread.turns, index, mode, encoder.tokenizer
                    )
                )
                inputs = tuple(unit.input for unit in units)
                validate_input_sources(inputs, owners, thread.thread, thread.turns)
                check_coverage(control, inputs, thread.thread, thread.turns)
                if set(owners) != {item.turn_id for item in inputs}:
                    raise ValueError(
                        "structured inputs do not cover every prompted turn"
                    )

                def progress(
                    done: int,
                    total: int,
                    index: int = thread.index,
                    selected: Mode = mode,
                ) -> None:
                    if done % 256 == 0 or done == total:
                        click.echo(
                            f"thread={index} mode={selected} inferred={done}/{total}"
                        )

                vectors, new = store.vectors(
                    tuple(item.content.text for item in inputs), encoder, progress
                )
                if (
                    vectors.shape != (len(inputs), 384)
                    or not np.isfinite(vectors).all()
                    or not np.allclose(
                        np.linalg.norm(vectors, axis=1), 1, atol=1e-9, rtol=0
                    )
                ):
                    raise ValueError("structured embedding matrix is invalid")
                name = f"{thread.index}-{mode}"
                matrix = output / f"{name}.npz"
                np.savez(
                    matrix,
                    vectors=vectors,
                    turn_ids=np.array([item.turn_id for item in inputs]),
                )
                artifact = StructuredArtifact(
                    thread_index=thread.index,
                    phase=entry.phase,
                    mode=mode,
                    turn_ids=owners,
                    units=units,
                    matrix_sha256=hashlib.sha256(matrix.read_bytes()).hexdigest(),
                    source_input_sha256=hashlib.sha256(source_bytes).hexdigest(),
                    seconds=time.perf_counter() - started,
                    new_vectors=new,
                    input_tokens=sum(item.tokens for item in inputs),
                )
                (output / f"{name}.json").write_bytes(
                    orjson.dumps(artifact.model_dump(mode="json"))
                )
                click.echo(
                    f"Built {name}: {artifact.seconds:.2f}s "
                    f"points={len(inputs)} new_vectors={new}"
                )
        (output / "complete").write_text("all registered structural artifacts built\n")
    finally:
        store.close()


if __name__ == "__main__":
    main()
