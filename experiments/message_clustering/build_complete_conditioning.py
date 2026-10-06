"""Condition all assistant chunks while retaining the frozen pooled/user vectors."""

import io
import os
import sqlite3
import time
from pathlib import Path

import click
import numpy as np
import orjson

from ablation_inputs import Conditioned, prepare
from build_complete_answers import Artifact, Contract, name
from build_token_weighting import digest
from corpus import Record, Role
from followup_inputs import AnswerScope, concat, lean_prompt
from local_encoder import Encoder
from run_complete_answers import Result
from structure_chunks import Mode, prepare_structured
from turn_artifact import SelectedText, Store, generation
from turn_study_corpus import Cohort, Phase

BASELINE_SHA = "7e104679c302eb34a8eaac3b35fba7f47e0d854f08938969861c05a072770a35"


class Details(Record):
    contract_sha256: str
    units: tuple[Conditioned, ...]
    original_chunks: int
    answer_chunks: int
    conditioned_chunks: int
    truncated_prompts: int
    seconds: float
    new_vectors: int


class AuditCatalogue(Record):
    contract_sha256: str
    files: dict[str, str]


def baseline(root: Path) -> Result:
    raw = (root / "results/all.json").read_bytes()
    if digest(raw) != BASELINE_SHA:
        raise ValueError("frozen baseline report changed")
    return Result.model_validate_json(raw)


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--reference", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--cache", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    parent: Path,
    reference: Path,
    protocol: Path,
    model: Path,
    cache: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    frozen = baseline(reference)
    source_root = reference / "artifacts"
    raw = (source_root / "contract.json").read_bytes()
    original_contract = Contract.model_validate_json(raw)
    if frozen.contract_sha256 != digest(raw):
        raise ValueError("frozen baseline contract changed")
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    if digest(cohort_raw) != original_contract.cohort_sha256:
        raise ValueError("cohort changed")
    cohort = Cohort.model_validate_json(cohort_raw)
    encoder = Encoder(model)
    if generation(encoder) != original_contract.encoder_generation:
        raise ValueError("encoder changed")
    contract = Contract(
        protocol_sha256=digest(protocol.read_bytes()),
        code_sha256={
            n: digest(Path(__file__).with_name(n).read_bytes())
            for n in (
                "build_complete_conditioning.py",
                "ablation_inputs.py",
                "followup_inputs.py",
                "structure_chunks.py",
                "build_structure_followup.py",
                "local_encoder.py",
                "turn_artifact.py",
            )
        },
        control_matrices_sha256={
            str(row.thread_index): frozen.artifact_sha256[
                name(row.thread_index, AnswerScope.ALL) + ".npz"
            ]
            for row in frozen.rows
        },
        scopes=(AnswerScope.ALL,),
    )
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    contract_raw = orjson.dumps(contract.model_dump(mode="json"))
    path = output / "contract.json"
    if path.exists() and path.read_bytes() != contract_raw:
        raise ValueError("resume contract changed")
    path.write_bytes(contract_raw)
    selected_generation = digest(
        (generation(encoder) + ":singleton-v1:character-overlap-pooling-v1").encode()
    )
    store = Store(output / "vectors.sqlite")
    try:
        with sqlite3.connect(cache.resolve().as_uri() + "?mode=ro", uri=True) as prior:
            with store.db:
                store.db.executemany(
                    "INSERT OR IGNORE INTO vectors VALUES (?,?,?)",
                    prior.execute(
                        "SELECT generation,text_hash,vector FROM vectors "
                        "WHERE generation=?",
                        (selected_generation,),
                    ),
                )
        for entry in cohort.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            captured = entry.captured
            filename = name(captured.index, AnswerScope.ALL)
            path = output / (filename + ".json")
            details_path = output / (filename + "-conditioning.json")
            original_raw = (source_root / (filename + ".json")).read_bytes()
            matrix_raw = (source_root / (filename + ".npz")).read_bytes()
            if (
                digest(original_raw) != frozen.artifact_sha256[filename + ".json"]
                or digest(matrix_raw) != frozen.artifact_sha256[filename + ".npz"]
            ):
                raise ValueError("baseline artifact changed")
            original = Artifact.model_validate_json(original_raw)
            if original.source_sha256 != digest(
                orjson.dumps(captured.model_dump(mode="json"))
            ):
                raise ValueError("history changed")
            if path.exists():
                saved = Artifact.model_validate_json(path.read_bytes())
                details = Details.model_validate_json(details_path.read_bytes())
                if (
                    saved.contract_sha256 != digest(contract_raw)
                    or details.contract_sha256 != saved.contract_sha256
                    or saved.matrix_sha256
                    != digest(path.with_suffix(".npz").read_bytes())
                    or saved.chunk_inputs != tuple(u.body for u in details.units)
                ):
                    raise ValueError("resume artifact changed")
                continue
            started = time.perf_counter()
            prepared = tuple(
                unit
                for index, turn in enumerate(captured.turns)
                if turn.prompts
                for unit in prepare_structured(
                    captured.thread,
                    captured.turns,
                    index,
                    Mode.HEADING,
                    encoder.tokenizer,
                    AnswerScope.ALL,
                )
            )
            if tuple(u.input for u in prepared) != original.chunk_inputs:
                raise ValueError("structural baseline inputs differ")
            units = prepare(
                prepared,
                captured.thread,
                captured.turns,
                encoder.tokenizer,
                AnswerScope.ALL,
            )
            with np.load(io.BytesIO(matrix_raw), allow_pickle=False) as archive:
                pooled = archive["pooled"].copy()
                pooled_owners = archive["pooled_owners"].copy()
                chunks = archive["chunks"].copy()
            retained = {
                item: vector
                for item, vector in zip(original.chunk_inputs, chunks, strict=True)
            }
            targets = tuple(
                i for i, unit in enumerate(units) if unit.selection is not None
            )
            vectors, added = store.selected_vectors(
                tuple(
                    SelectedText(
                        text=units[i].content.text, selection=units[i].selection
                    )
                    for i in targets
                ),
                encoder,
                lambda *_: None,
                singleton=True,
            )
            by_target = dict(zip(targets, vectors, strict=True))
            conditioned = np.stack(
                [
                    by_target[i] if i in by_target else retained[unit.body]
                    for i, unit in enumerate(units)
                ]
            )
            np.savez_compressed(
                path.with_suffix(".npz"),
                pooled=pooled,
                pooled_owners=pooled_owners,
                chunks=conditioned,
                chunk_owners=np.array([u.body.turn_id for u in units]),
            )
            prompts = tuple(
                concat(tuple(lean_prompt(captured.thread, i) for i in turn.prompts))
                for turn in captured.turns
                if turn.prompts
            )
            seconds = time.perf_counter() - started
            details = Details(
                contract_sha256=digest(contract_raw),
                units=units,
                original_chunks=len(prepared),
                answer_chunks=sum(u.body.role == Role.ASSISTANT for u in units),
                conditioned_chunks=len(targets),
                truncated_prompts=sum(
                    len(encoder.tokenizer.encode(p.text, add_special_tokens=False).ids)
                    > 80
                    for p in prompts
                ),
                seconds=seconds,
                new_vectors=added,
            )
            details_path.write_bytes(orjson.dumps(details.model_dump(mode="json")))
            built = original.model_copy(
                update={
                    "contract_sha256": digest(contract_raw),
                    "matrix_sha256": digest(path.with_suffix(".npz").read_bytes()),
                    "chunk_inputs": tuple(u.body for u in units),
                    "seconds": seconds,
                    "new_vectors": added,
                    "tokens": sum(i.tokens for i in original.pooled_inputs)
                    + sum(
                        len(encoder.tokenizer.encode(u.content.text).ids) for u in units
                    ),
                }
            )
            path.write_bytes(orjson.dumps(built.model_dump(mode="json")))
            click.echo(
                f"history={captured.index} chunks={len(units)} "
                f"new={added} seconds={seconds:.2f}",
                err=True,
            )
        catalogue = AuditCatalogue(
            contract_sha256=digest(contract_raw),
            files={
                path.name: digest(path.read_bytes())
                for path in sorted(output.glob("*-conditioning.json"))
            },
        )
        if len(catalogue.files) != 160:
            raise ValueError("conditioning audit incomplete")
        catalogue_path = output / "audit-catalogue.json"
        catalogue_raw = orjson.dumps(catalogue.model_dump(mode="json"))
        if catalogue_path.exists() and catalogue_path.read_bytes() != catalogue_raw:
            raise ValueError("conditioning catalogue changed")
        catalogue_path.write_bytes(catalogue_raw)
    finally:
        store.close()


if __name__ == "__main__":
    main()
