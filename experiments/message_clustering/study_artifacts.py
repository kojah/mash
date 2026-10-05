"""Build the five registered views without reading annotations or scoring clusters."""

import hashlib
import os
import sqlite3
import time
from enum import StrEnum
from pathlib import Path

import click
import numpy as np
import orjson
from tokenizers import Tokenizer

from clustering import decode_stored_vector, normalized_vectors
from corpus import Record, Role, Thread, digest
from local_encoder import Encoder
from turn_artifact import Artifact, Store, generation
from turn_study_corpus import Cohort, Phase
from turns import Recipe, Turn, Unit, joined, units, windows


class View(StrEnum):
    PROMPT = "prompt_only"
    JOINT = "joint_truncation"
    PASSAGES = "prompt_conditioned_passages"
    POOLED = "pooled_message_control"
    SEPARATE = "separate_view_pooling"


class Cost(Record):
    thread_index: int
    phase: Phase
    view: View
    seconds: float
    input_units: int
    clustering_points: int
    new_vectors: int
    input_tokens: int
    discarded_tokens: int


def separate_units(
    thread: Thread, turn: Turn, tokenizer: Tokenizer
) -> tuple[Unit, ...]:
    result = []
    for role, sources in ((Role.USER, turn.prompts), (Role.ASSISTANT, turn.answers)):
        for index, (text, spans) in enumerate(
            windows(tokenizer, *joined(thread, sources), 510)
        ):
            result.append(
                Unit(
                    id=digest(f"{turn.id}:separate:{role}:{index}"),
                    turn_id=turn.id,
                    text=text,
                    spans=spans,
                    represented_sources=tuple(sorted({s.source for s in spans})),
                    tokens=len(tokenizer.encode(text).ids),
                )
            )
    return tuple(result)


def separate_pool(
    turns: tuple[Turn, ...], prepared: tuple[Unit, ...], vectors: np.ndarray
) -> np.ndarray:
    """Equal view weights rather than answer-length weights; missing view abstains."""
    pooled = []
    for turn in turns:
        views = []
        for sources in (set(turn.prompts), set(turn.answers)):
            indices = [
                i
                for i, unit in enumerate(prepared)
                if unit.turn_id == turn.id and set(unit.represented_sources) & sources
            ]
            if indices:
                views.append(normalized_vectors(vectors[indices].mean(axis=0)[None])[0])
        if not views:
            raise ValueError("turn has no separate-view embedding inputs")
        pooled.append(np.stack(views).mean(axis=0))
    return normalized_vectors(np.stack(pooled))


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
@click.option("--cache", type=click.Path(path_type=Path, exists=True))
def main(
    cohort: Path, protocol: Path, model: Path, output: Path, cache: Path | None
) -> None:
    os.umask(0o077)
    data = cohort.read_bytes()
    captured = Cohort.model_validate_json(data)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    start = time.perf_counter()
    encoder = Encoder(model)
    click.echo(f"Encoder startup: {time.perf_counter() - start:.3f}s")
    (output / "encoder.json").write_bytes(orjson.dumps(encoder.manifest.model_dump()))
    (output / "cohort.sha256").write_text(hashlib.sha256(data).hexdigest())
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    (output / "protocol.sha256").write_text(protocol_sha)
    store = Store(output / "gold.sqlite")
    try:
        if cache:
            prior = sqlite3.connect(cache.resolve().as_uri() + "?mode=ro", uri=True)
            try:
                with store.db:
                    store.db.executemany(
                        "INSERT OR IGNORE INTO vectors VALUES (?,?,?)",
                        prior.execute(
                            "SELECT generation,text_hash,vector FROM vectors"
                        ),
                    )
            finally:
                prior.close()
        for entry in captured.entries:
            thread = entry.captured.thread
            turns = tuple(t for t in entry.captured.turns if t.prompts)
            for view in View:
                started = time.perf_counter()
                new = 0
                if view == View.POOLED:
                    prepared = tuple(
                        Unit(
                            id=t.id,
                            turn_id=t.id,
                            text="",
                            spans=(),
                            represented_sources=t.sources,
                            tokens=0,
                        )
                        for t in turns
                    )
                    vectors = normalized_vectors(
                        np.stack(
                            [
                                np.stack(
                                    [
                                        decode_stored_vector(
                                            bytes.fromhex(
                                                thread.messages[i].vector_hex
                                            ),
                                            thread.messages[i].dimensions,
                                            thread.messages[i].norm,
                                        )
                                        for i in t.sources
                                    ]
                                ).mean(axis=0)
                                for t in turns
                            ]
                        )
                    )
                else:
                    prepared = tuple(
                        unit
                        for turn in turns
                        for unit in (
                            separate_units(thread, turn, encoder.tokenizer)
                            if view == View.SEPARATE
                            else units(
                                thread, turn, Recipe(view.value), encoder.tokenizer
                            )
                        )
                    )

                    def progress(
                        done: int,
                        total: int,
                        thread_index: int = entry.captured.index,
                        selected_view: View = view,
                    ) -> None:
                        if done % 256 == 0 or done == total:
                            click.echo(
                                f"thread={thread_index} view={selected_view} "
                                f"inferred={done}/{total}"
                            )

                    vectors, new = store.vectors(
                        tuple(u.text for u in prepared), encoder, progress
                    )
                points = (
                    separate_pool(turns, prepared, vectors)
                    if view == View.SEPARATE
                    else vectors
                )
                owners = (
                    tuple(t.id for t in turns)
                    if view == View.SEPARATE
                    else tuple(u.turn_id for u in prepared)
                )
                name = f"{entry.captured.index}-{view.value}"
                np.savez(
                    output / f"{name}.npz", vectors=points, turn_ids=np.array(owners)
                )
                store.publish(
                    name,
                    Artifact(
                        ancestry_sha256=entry.captured.source_sha256,
                        source_sha256=hashlib.sha256(data).hexdigest(),
                        protocol_sha256=protocol_sha,
                        encoder_generation=generation(encoder),
                        turns=turns,
                        units=prepared,
                    ),
                )
                cost = Cost(
                    thread_index=entry.captured.index,
                    phase=entry.phase,
                    view=view,
                    seconds=time.perf_counter() - started,
                    input_units=len(prepared),
                    clustering_points=len(points),
                    new_vectors=new,
                    input_tokens=sum(u.tokens for u in prepared),
                    discarded_tokens=sum(u.discarded_tokens for u in prepared),
                )
                with (output / "costs.jsonl").open("ab") as file:
                    file.write(orjson.dumps(cost.model_dump(mode="json")) + b"\n")
                click.echo(f"Built {name}: {cost.seconds:.2f}s new_vectors={new}")
    finally:
        store.close()


if __name__ == "__main__":
    main()
