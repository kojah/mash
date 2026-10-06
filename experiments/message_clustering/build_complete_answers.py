"""Rebuild complete-answer controls with singleton inference and source audits."""

import os
import time
from pathlib import Path

import click
import numpy as np
import orjson

from build_input_followup import Built, pool
from build_local_representation import COHORT_SHA, MODEL_GENERATION, source
from build_structure_followup import check_coverage, own_mask
from build_token_weighting import digest
from corpus import Record
from followup_inputs import AnswerScope, Input, Variant, prepare
from freeze_link_review import SOURCE_SHA
from local_encoder import Encoder
from run_turn_ablation import AblationReport
from structure_chunks import Mode, prepare_structured
from turn_artifact import Store, generation
from turn_study_corpus import Cohort, Phase


class Contract(Record):
    cohort_sha256: str = COHORT_SHA
    encoder_generation: str = MODEL_GENERATION
    protocol_sha256: str
    code_sha256: dict[str, str]
    control_matrices_sha256: dict[str, str]
    scopes: tuple[AnswerScope, ...] = (AnswerScope.LAST, AnswerScope.ALL)


class Artifact(Record):
    thread_index: int
    scope: AnswerScope
    source_sha256: str
    contract_sha256: str
    matrix_sha256: str
    pooled_inputs: tuple[Input, ...]
    chunk_inputs: tuple[Input, ...]
    seconds: float
    new_vectors: int
    tokens: int
    represented_answer_sources: int
    available_answer_sources: int


def name(index: int, scope: AnswerScope) -> str:
    return f"{index}-{scope}"


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--control", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--cache", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    parent: Path, protocol: Path, model: Path, control: Path, cache: Path, output: Path
) -> None:
    import sqlite3

    os.umask(0o077)
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    report_raw = (
        parent / "turn-three-factor-ablation-20261006/development.json"
    ).read_bytes()
    if digest(cohort_raw) != COHORT_SHA or digest(report_raw) != SOURCE_SHA:
        raise ValueError("frozen parents changed")
    cohort = Cohort.model_validate_json(cohort_raw)
    report = AblationReport.model_validate_json(report_raw)
    encoder = Encoder(model)
    if generation(encoder) != MODEL_GENERATION:
        raise ValueError("installed encoder changed")
    entries = tuple(e for e in cohort.entries if e.phase == Phase.DEVELOPMENT)
    contract = Contract(
        protocol_sha256=digest(protocol.read_bytes()),
        code_sha256={
            n: digest(Path(__file__).with_name(n).read_bytes())
            for n in (
                "build_complete_answers.py",
                "followup_inputs.py",
                "structure_chunks.py",
                "build_structure_followup.py",
                "local_encoder.py",
                "turn_artifact.py",
            )
        },
        control_matrices_sha256={
            str(e.captured.index): digest(
                (control / f"{e.captured.index}.npz").read_bytes()
            )
            for e in entries
        },
    )
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    contract_raw = orjson.dumps(contract.model_dump(mode="json"))
    path = output / "contract.json"
    if path.exists() and path.read_bytes() != contract_raw:
        raise ValueError("resume contract changed")
    path.write_bytes(contract_raw)
    store = Store(output / "vectors.sqlite")
    try:
        with sqlite3.connect(cache.resolve().as_uri() + "?mode=ro", uri=True) as prior:
            with store.db:
                store.db.executemany(
                    "INSERT OR IGNORE INTO vectors VALUES (?,?,?)",
                    prior.execute(
                        "SELECT generation,text_hash,vector FROM vectors "
                        "WHERE generation=?",
                        (MODEL_GENERATION + ":singleton-v1",),
                    ),
                )
        for entry in entries:
            captured = entry.captured
            thread = captured.thread
            turns = tuple(t for t in captured.turns if t.prompts)
            legacy, _, _, _ = source(entry, parent, report)
            old_pooled = Built.model_validate_json(
                (
                    parent
                    / "turn-input-followup-20261005/artifacts"
                    / f"{captured.index}-context_whole.json"
                ).read_bytes()
            )
            for scope in contract.scopes:
                path = output / (name(captured.index, scope) + ".json")
                if path.exists():
                    saved = Artifact.model_validate_json(path.read_bytes())
                    if (
                        saved.contract_sha256 != digest(contract_raw)
                        or saved.matrix_sha256
                        != digest(path.with_suffix(".npz").read_bytes())
                        or saved.source_sha256
                        != digest(orjson.dumps(captured.model_dump(mode="json")))
                    ):
                        raise ValueError("resume artifact changed")
                    continue
                started = time.perf_counter()
                pooled = tuple(
                    i
                    for index, t in enumerate(captured.turns)
                    if t.prompts
                    for i in prepare(
                        thread,
                        captured.turns,
                        index,
                        Variant.CONTEXT_MEAN,
                        encoder.tokenizer,
                        scope,
                    )
                )
                chunks = tuple(
                    u.input
                    for index, t in enumerate(captured.turns)
                    if t.prompts
                    for u in prepare_structured(
                        thread,
                        captured.turns,
                        index,
                        Mode.HEADING,
                        encoder.tokenizer,
                        scope,
                    )
                )
                if scope == AnswerScope.LAST and (
                    pooled != old_pooled.inputs
                    or chunks != tuple(u.input for u in legacy.units)
                ):
                    raise ValueError("last-only control input changed")
                check_coverage(pooled, chunks, thread, captured.turns, scope)
                if scope == AnswerScope.ALL:
                    for turn in turns:
                        for index in turn.answers:
                            text = thread.messages[index].projection
                            required = np.fromiter(
                                (not c.isspace() for c in text), dtype=np.bool_
                            )
                            mask = own_mask(pooled, turn.id, index, len(text))
                            if not np.all(mask[required]):
                                raise ValueError(
                                    "whole-view answer source coverage is incomplete"
                                )
                if any(i.tokens > 512 for i in pooled + chunks):
                    raise ValueError("input exceeds encoder budget")
                vectors, new = store.vectors(
                    tuple(i.content.text for i in pooled + chunks),
                    encoder,
                    lambda *_: None,
                    singleton=True,
                )
                a = pool(turns, pooled, vectors[: len(pooled)])
                b = vectors[len(pooled) :]
                a_owners = tuple(t.id for t in turns)
                b_owners = tuple(i.turn_id for i in chunks)
                if scope == AnswerScope.LAST:
                    prior_path = control / f"{captured.index}.npz"
                    if (
                        digest(prior_path.read_bytes())
                        != contract.control_matrices_sha256[str(captured.index)]
                    ):
                        raise ValueError("matched control changed")
                    with np.load(prior_path, allow_pickle=False) as previous:
                        np.testing.assert_array_equal(a, previous["pooled_uniform"])
                        np.testing.assert_array_equal(b, previous["chunks_uniform"])
                        if a_owners != tuple(
                            previous["pooled_owners"].tolist()
                        ) or b_owners != tuple(previous["chunk_owners"].tolist()):
                            raise ValueError("control point ownership changed")
                np.savez_compressed(
                    path.with_suffix(".npz"),
                    pooled=a,
                    chunks=b,
                    pooled_owners=np.array(a_owners),
                    chunk_owners=np.array(b_owners),
                )
                available = {
                    (t.id, s)
                    for t in turns
                    for s in t.answers
                    if thread.messages[s].projection.strip()
                }
                represented = {
                    (i.turn_id, location.source.source)
                    for i in chunks
                    for location in i.content.locations
                    if (i.turn_id, location.source.source) in available
                }
                built = Artifact(
                    thread_index=captured.index,
                    scope=scope,
                    source_sha256=digest(
                        orjson.dumps(captured.model_dump(mode="json"))
                    ),
                    contract_sha256=digest(contract_raw),
                    matrix_sha256=digest(path.with_suffix(".npz").read_bytes()),
                    pooled_inputs=pooled,
                    chunk_inputs=chunks,
                    seconds=time.perf_counter() - started,
                    new_vectors=new,
                    tokens=sum(i.tokens for i in pooled + chunks),
                    represented_answer_sources=len(represented),
                    available_answer_sources=len(available),
                )
                path.write_bytes(orjson.dumps(built.model_dump(mode="json")))
                click.echo(
                    f"history={captured.index} scope={scope} new={new} "
                    f"answers={len(represented)}/{len(available)} "
                    f"seconds={built.seconds:.2f}",
                    err=True,
                )
    finally:
        store.close()


if __name__ == "__main__":
    main()
