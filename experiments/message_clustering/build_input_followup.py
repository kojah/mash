"""Build frozen follow-up vectors and exact input provenance without labels."""

import hashlib
import os
import sqlite3
import time
from pathlib import Path

import click
import numpy as np
import orjson

from clustering import VectorMatrix, normalized_vectors
from corpus import Record, Role
from followup_inputs import AnswerScope, Input, Variant, prepare
from local_encoder import Encoder
from study_artifacts import View
from turn_artifact import SelectedText, Store, generation
from turn_study_corpus import Cohort, Phase
from turns import Turn


class Built(Record):
    thread_index: int
    phase: Phase
    variant: Variant
    seconds: float
    new_vectors: int
    input_tokens: int
    matrix_sha256: str
    frozen_control_sha256: str | None = None
    turn_ids: tuple[str, ...]
    inputs: tuple[Input, ...]


def pool(
    turns: tuple[Turn, ...], inputs: tuple[Input, ...], vectors: VectorMatrix
) -> VectorMatrix:
    """Normalize each role's mean, then combine available roles equally."""
    if len(inputs) != len(vectors):
        raise ValueError("each follow-up input needs a vector")
    result = []
    for turn in turns:
        views = []
        for role in (Role.USER, Role.ASSISTANT):
            indices = [
                i
                for i, item in enumerate(inputs)
                if item.turn_id == turn.id and item.role == role
            ]
            if indices:
                views.append(normalized_vectors(vectors[indices].mean(axis=0)[None])[0])
        if not views:
            raise ValueError(f"turn has no usable input: {turn.id}")
        result.append(np.stack(views).mean(axis=0))
    return normalized_vectors(np.stack(result))


def import_cache(store: Store, path: Path) -> None:
    prior = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)
    try:
        with store.db:
            store.db.executemany(
                "INSERT OR IGNORE INTO vectors VALUES (?,?,?)",
                prior.execute("SELECT generation,text_hash,vector FROM vectors"),
            )
    finally:
        prior.close()


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
        raise ValueError("original artifacts belong to a different cohort")
    captured = Cohort.model_validate_json(data)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    encoder = Encoder(model)
    old_encoder = (original / "encoder.json").read_bytes()
    if encoder.manifest != type(encoder.manifest).model_validate_json(old_encoder):
        raise ValueError("raw control and new variants must use the same encoder")
    (output / "encoder.json").write_bytes(old_encoder)
    (output / "cohort.sha256").write_text(cohort_sha)
    (output / "protocol.sha256").write_text(
        hashlib.sha256(protocol.read_bytes()).hexdigest()
    )
    (output / "pooling-generation.txt").write_text(generation(encoder))
    store = Store(output / "vectors.sqlite")
    try:
        import_cache(store, original / "gold.sqlite")
        for entry in captured.entries:
            thread = entry.captured.thread
            all_turns = entry.captured.turns
            turns = tuple(turn for turn in all_turns if turn.prompts)
            owners = tuple(turn.id for turn in turns)
            for variant in Variant:
                started = time.perf_counter()
                control_sha = None
                inputs: tuple[Input, ...] = ()
                new = 0
                if variant == Variant.RAW:
                    control = original / f"{entry.captured.index}-{View.SEPARATE}.npz"
                    control_sha = hashlib.sha256(control.read_bytes()).hexdigest()
                    with np.load(control, allow_pickle=False) as frozen:
                        if tuple(frozen["turn_ids"].tolist()) != owners:
                            raise ValueError("raw control turn identities differ")
                        points = frozen["vectors"].copy()
                else:
                    inputs = tuple(
                        item
                        for index, turn in enumerate(all_turns)
                        if turn.prompts
                        for item in prepare(
                            thread,
                            all_turns,
                            index,
                            variant,
                            encoder.tokenizer,
                            AnswerScope.LAST,
                        )
                    )

                    def progress(
                        done: int,
                        total: int,
                        index: int = entry.captured.index,
                        selected: Variant = variant,
                    ) -> None:
                        if done % 256 == 0 or done == total:
                            click.echo(
                                f"thread={index} variant={selected} "
                                f"inferred={done}/{total}"
                            )

                    vectors, new = store.selected_vectors(
                        tuple(
                            SelectedText(text=item.content.text, selection=item.target)
                            for item in inputs
                        ),
                        encoder,
                        progress,
                    )
                    points = pool(turns, inputs, vectors)
                if points.shape != (len(turns), 384) or not np.isfinite(points).all():
                    raise ValueError("invalid follow-up matrix")
                name = f"{entry.captured.index}-{variant}"
                matrix = output / f"{name}.npz"
                np.savez(matrix, vectors=points, turn_ids=np.array(owners))
                built = Built(
                    thread_index=entry.captured.index,
                    phase=entry.phase,
                    variant=variant,
                    seconds=time.perf_counter() - started,
                    new_vectors=new,
                    input_tokens=sum(item.tokens for item in inputs),
                    matrix_sha256=hashlib.sha256(matrix.read_bytes()).hexdigest(),
                    frozen_control_sha256=control_sha,
                    turn_ids=owners,
                    inputs=inputs,
                )
                (output / f"{name}.json").write_bytes(
                    orjson.dumps(built.model_dump(mode="json"))
                )
                click.echo(f"Built {name}: {built.seconds:.2f}s new_vectors={new}")
        (output / "complete").write_text("all cohort variants built\n")
    finally:
        store.close()


if __name__ == "__main__":
    main()
