"""Build six frozen per-history representations without annotation access."""

import hashlib
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson

from ablation_inputs import Conditioned
from build_input_followup import import_cache
from build_structure_followup import StructuredArtifact
from build_turn_ablation import Artifact as ConditionedArtifact
from corpus import Record, Role
from experiment_study import Context, Contract, Setting
from freeze_link_review import SOURCE_SHA
from local_encoder import Encoder
from local_representation import inputs, pooling
from run_turn_ablation import AblationReport, matrix
from turn_artifact import Store, generation
from turn_study_corpus import Cohort, Entry, Phase

COHORT_SHA = "d779a9a1850cb29558987e510ddd2452878aca20ebb57dd12ee5c657d5192805"
MODEL_GENERATION = "2894b8dc20a8bea9f9f78c49d9e349728c69450a273a509298ec8aaf94905491"
SETTINGS = tuple(
    Setting(context=c, boilerplate=b) for c in Context for b in (False, True)
)


class Artifact(Record):
    setting: Setting
    thread_index: int
    protocol_sha256: str
    cohort_sha256: str = COHORT_SHA
    encoder_generation: str = MODEL_GENERATION
    units: tuple[Conditioned, ...]
    matrix_sha256: str
    seconds: float
    new_vectors: int
    candidate_vectors: int
    accepted_contexts: int
    weighted_points: int
    weighted_tokens: int


def name(index: int, setting: Setting) -> str:
    return f"{index}-{setting.context.value}-{int(setting.boilerplate)}"


def pinned(path: Path, key: str, report: AblationReport) -> bytes:
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != report.report.artifact_sha256[key]:
        raise ValueError("parent input/vector changed")
    return raw


def source(
    entry: Entry, parent: Path, report: AblationReport
) -> tuple[StructuredArtifact, np.ndarray, ConditionedArtifact, np.ndarray]:
    index = entry.captured.index
    base = parent / "turn-structure-followup-20261005/artifacts-restart1"
    first = parent / "turn-three-factor-ablation-20261006/artifacts"
    filename = f"{index}-structure_heading.json"
    structured = StructuredArtifact.model_validate_json(
        pinned(base / filename, f"structural/{filename}", report)
    )
    original, owners = matrix(
        pinned(
            base / f"{index}-structure_heading.npz", f"structural/{index}.npz", report
        )
    )
    conditioned = ConditionedArtifact.model_validate_json(
        pinned(first / f"{index}.json", f"conditioned/{index}.json", report)
    )
    conditioned_vectors, conditioned_owners = matrix(
        pinned(first / f"{index}.npz", f"conditioned/{index}.npz", report)
    )
    if (
        owners != tuple(u.input.turn_id for u in structured.units)
        or conditioned_owners != tuple(u.body.turn_id for u in conditioned.units)
        or structured.phase != Phase.DEVELOPMENT
        or structured.thread_index != index
        or conditioned.thread_index != index
    ):
        raise ValueError("parent ownership/phase differs")
    return structured, original, conditioned, conditioned_vectors


def build(
    entry: Entry,
    parent: Path,
    report: AblationReport,
    output: Path,
    contract: Contract,
    encoder: Encoder,
    store: Store,
) -> None:
    structured, original, first, _ = source(entry, parent, report)
    captured = entry.captured
    anchor_started = time.perf_counter()
    assistant_rows = tuple(
        i for i, u in enumerate(structured.units) if u.input.role == Role.ASSISTANT
    )
    anchor = original.copy()
    anchor_vectors, anchor_added = store.vectors(
        tuple(structured.units[i].input.content.text for i in assistant_rows),
        encoder,
        lambda *_: None,
        singleton=True,
    )
    anchor[list(assistant_rows)] = anchor_vectors
    anchor_seconds = time.perf_counter() - anchor_started
    for context in Context:
        paths = tuple(
            output
            / (name(captured.index, Setting(context=context, boilerplate=b)) + ".json")
            for b in (False, True)
        )
        if all(p.exists() for p in paths):
            for path in paths:
                saved = Artifact.model_validate_json(path.read_bytes())
                if saved.protocol_sha256 != contract.protocol_sha256:
                    raise ValueError("resume protocol differs")
                if (
                    hashlib.sha256(path.with_suffix(".npz").read_bytes()).hexdigest()
                    != saved.matrix_sha256
                ):
                    raise ValueError("resume matrix differs")
            continue
        started = time.perf_counter()
        units, candidate_vectors, accepted = inputs(
            context,
            structured.units,
            anchor,
            captured.thread,
            captured.turns,
            encoder,
            store,
        )
        added = 0
        if context == Context.NONE:
            off = anchor
            added = anchor_added
        else:
            if context == Context.FIRST and units != first.units:
                raise ValueError("first80 input differs from frozen C treatment")
            answer_rows = tuple(
                i for i, u in enumerate(units) if u.body.role == Role.ASSISTANT
            )
            answer_vectors, added = store.selected_vectors(
                tuple(pooling(units[i], False) for i in answer_rows),
                encoder,
                lambda *_: None,
                singleton=True,
            )
            off = np.empty((len(units), 384), dtype=np.float64)
            off[list(answer_rows)] = answer_vectors
            # Exact user-vector reuse avoids batch-padding numerical drift.
            user_vectors = [
                v
                for u, v in zip(structured.units, original, strict=True)
                if u.input.role == Role.USER
            ]
            user_rows = [i for i, u in enumerate(units) if u.body.role == Role.USER]
            if len(user_vectors) != len(user_rows):
                raise ValueError("user coverage changed")
            for row, vector in zip(user_rows, user_vectors, strict=True):
                off[row] = vector
        for boilerplate, path in zip((False, True), paths, strict=True):
            vectors = off.copy()
            requests = tuple(pooling(u, boilerplate) for u in units)
            changed = tuple(i for i, item in enumerate(requests) if item.downweighted)
            new_weighted = 0
            if changed:
                modified, new_weighted = store.selected_vectors(
                    tuple(requests[i] for i in changed),
                    encoder,
                    lambda *_: None,
                    singleton=True,
                )
                vectors[list(changed)] = modified
            np.savez_compressed(
                path.with_suffix(".npz"),
                vectors=vectors,
                turn_ids=np.array([u.body.turn_id for u in units]),
            )
            artifact = Artifact(
                setting=Setting(context=context, boilerplate=boilerplate),
                thread_index=captured.index,
                protocol_sha256=contract.protocol_sha256,
                units=units,
                matrix_sha256=hashlib.sha256(
                    path.with_suffix(".npz").read_bytes()
                ).hexdigest(),
                seconds=(
                    time.perf_counter()
                    - started
                    + (anchor_seconds if context != Context.FIRST else 0)
                ),
                new_vectors=added + new_weighted,
                candidate_vectors=candidate_vectors,
                accepted_contexts=accepted,
                weighted_points=len(changed),
                weighted_tokens=sum(
                    sum(
                        any(left < s.end and right > s.start for s in item.downweighted)
                        for left, right in encoder.tokenizer.encode(item.text).offsets
                        if right > left
                    )
                    for item in requests
                    if item.downweighted
                ),
            )
            path.write_bytes(orjson.dumps(artifact.model_dump(mode="json")))
        click.echo(f"history {captured.index}: {context.value} complete", err=True)


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(parent: Path, protocol: Path, model: Path, output: Path) -> None:
    os.umask(0o077)
    raw = (parent / "turn-study-fleet-verified-20261005/cohort.json").read_bytes()
    if hashlib.sha256(raw).hexdigest() != COHORT_SHA:
        raise ValueError("cohort changed")
    cohort = Cohort.model_validate_json(raw)
    report_raw = (
        parent / "turn-three-factor-ablation-20261006/development.json"
    ).read_bytes()
    if hashlib.sha256(report_raw).hexdigest() != SOURCE_SHA:
        raise ValueError("parent report changed")
    report = AblationReport.model_validate_json(report_raw)
    encoder = Encoder(model)
    if generation(encoder) != MODEL_GENERATION:
        raise ValueError("encoder changed")
    contract = Contract(
        protocol_sha256=hashlib.sha256(protocol.read_bytes()).hexdigest(),
        cohort_sha256=COHORT_SHA,
        encoder_generation=MODEL_GENERATION,
        settings=SETTINGS,
    )
    entries = tuple(e for e in cohort.entries if e.phase == Phase.DEVELOPMENT)
    # Validate every parent before any new inference, including resumed builds.
    for entry in entries:
        source(entry, parent, report)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    contract_path = output / "contract.json"
    if (
        contract_path.exists()
        and Contract.model_validate_json(contract_path.read_bytes()) != contract
    ):
        raise ValueError("build contract changed")
    contract_path.write_bytes(orjson.dumps(contract.model_dump(mode="json")))
    store = Store(output / "vectors.sqlite")
    try:
        import_cache(
            store,
            parent / "turn-three-factor-ablation-20261006/artifacts/vectors.sqlite",
        )
        import_cache(
            store, parent / "turn-input-followup-20261005/artifacts/vectors.sqlite"
        )
        for entry in entries:
            build(entry, parent, report, output, contract, encoder, store)
    finally:
        store.close()


if __name__ == "__main__":
    main()
