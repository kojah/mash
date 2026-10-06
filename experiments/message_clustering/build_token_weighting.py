"""Build the frozen token-pooling comparison without reading pair judgments."""

import hashlib
import os
import time
from pathlib import Path

import click
import numpy as np
import orjson

from build_input_followup import Built, pool
from build_local_representation import COHORT_SHA, MODEL_GENERATION, source
from corpus import Record
from freeze_link_review import SOURCE_SHA
from local_encoder import Encoder, Selection
from run_turn_ablation import AblationReport
from token_weighting import Frequencies, Policy, encode_policies
from turn_artifact import generation
from turn_study_corpus import Cohort, Phase


class Contract(Record):
    protocol_sha256: str
    cohort_sha256: str = COHORT_SHA
    encoder_generation: str = MODEL_GENERATION
    code_sha256: dict[str, str]
    pooled_input_sha256: dict[str, str]
    policies: tuple[Policy, ...] = tuple(Policy)


class Artifact(Record):
    thread_index: int
    contract_sha256: str
    source_sha256: str
    matrix_sha256: str
    seconds: float
    inferred_inputs: int
    pooled_inputs: int
    chunk_inputs: int
    tokens: int
    weighted_tokens: dict[Policy, int]
    changed_vectors: dict[Policy, int]


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(parent: Path, protocol: Path, model: Path, output: Path) -> None:
    os.umask(0o077)
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    report_raw = (
        parent / "turn-three-factor-ablation-20261006/development.json"
    ).read_bytes()
    if digest(cohort_raw) != COHORT_SHA or digest(report_raw) != SOURCE_SHA:
        raise ValueError("frozen parent changed")
    cohort = Cohort.model_validate_json(cohort_raw)
    report = AblationReport.model_validate_json(report_raw)
    encoder = Encoder(model)
    if generation(encoder) != MODEL_GENERATION:
        raise ValueError("installed encoder changed")
    contract = Contract(
        protocol_sha256=digest(protocol.read_bytes()),
        pooled_input_sha256={
            str(e.captured.index): digest(
                (
                    parent
                    / "turn-input-followup-20261005/artifacts"
                    / f"{e.captured.index}-context_whole.json"
                ).read_bytes()
            )
            for e in cohort.entries
            if e.phase == Phase.DEVELOPMENT
        },
        code_sha256={
            name: digest(Path(__file__).with_name(name).read_bytes())
            for name in (
                "build_token_weighting.py",
                "token_weighting.py",
                "local_encoder.py",
            )
        },
    )
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    contract_bytes = orjson.dumps(contract.model_dump(mode="json"))
    contract_path = output / "contract.json"
    if contract_path.exists() and contract_path.read_bytes() != contract_bytes:
        raise ValueError("resume contract differs")
    contract_path.write_bytes(contract_bytes)
    contract_sha = digest(contract_bytes)
    for entry in cohort.entries:
        if entry.phase != Phase.DEVELOPMENT:
            continue
        captured = entry.captured
        path = output / f"{captured.index}.json"
        if path.exists():
            saved = Artifact.model_validate_json(path.read_bytes())
            if (
                saved.contract_sha256 != contract_sha
                or saved.matrix_sha256 != digest(path.with_suffix(".npz").read_bytes())
                or saved.source_sha256
                != digest(orjson.dumps(captured.model_dump(mode="json")))
            ):
                raise ValueError("resume artifact differs")
            continue
        started = time.perf_counter()
        structured, _, _, _ = source(entry, parent, report)
        pooled_path = (
            parent
            / "turn-input-followup-20261005/artifacts"
            / f"{captured.index}-context_whole.json"
        )
        pooled_raw = pooled_path.read_bytes()
        if digest(pooled_raw) != contract.pooled_input_sha256[str(captured.index)]:
            raise ValueError("pooled inputs changed")
        pooled = Built.model_validate_json(pooled_raw)
        if (
            pooled.matrix_sha256
            != report.report.artifact_sha256[
                f"pooled/{pooled_path.with_suffix('.npz').name}"
            ]
            or pooled.phase != Phase.DEVELOPMENT
            or pooled.thread_index != captured.index
        ):
            raise ValueError("pooled source provenance changed")
        turns = tuple(t for t in captured.turns if t.prompts)
        if pooled.turn_ids != tuple(t.id for t in turns):
            raise ValueError("pooled ownership differs")
        requests = tuple((i.content.text, i.target) for i in pooled.inputs) + tuple(
            (u.input.content.text, None) for u in structured.units
        )
        frequency = Frequencies.from_messages(
            tuple(m.projection for m in captured.thread.messages)
        )
        memo: dict[tuple[str, Selection | None], dict[Policy, np.ndarray]] = {}
        weighted = dict.fromkeys(Policy, 0)
        points: dict[Policy, list[np.ndarray]] = {p: [] for p in Policy}
        tokens = 0
        for text, selection in requests:
            key = (text, selection)
            if key not in memo:
                vectors, coverage = encode_policies(encoder, text, frequency, selection)
                memo[key] = vectors
                for policy in Policy:
                    weighted[policy] += coverage[policy]
                tokens += len(encoder.tokenizer.encode(text).ids)
            for policy in Policy:
                points[policy].append(memo[key][policy])
        matrices = {}
        changed = {}
        for policy in Policy:
            values = np.stack(points[policy])
            control = np.stack(points[Policy.UNIFORM])
            changed[policy] = int(np.count_nonzero(np.any(values != control, axis=1)))
            matrices[f"pooled_{policy}"] = pool(
                turns, pooled.inputs, values[: len(pooled.inputs)]
            )
            matrices[f"chunks_{policy}"] = values[len(pooled.inputs) :]
        np.savez_compressed(
            path.with_suffix(".npz"),
            allow_pickle=False,
            **matrices,
            pooled_owners=np.array(pooled.turn_ids),
            chunk_owners=np.array([u.input.turn_id for u in structured.units]),
        )
        artifact = Artifact(
            thread_index=captured.index,
            contract_sha256=contract_sha,
            source_sha256=digest(orjson.dumps(captured.model_dump(mode="json"))),
            matrix_sha256=digest(path.with_suffix(".npz").read_bytes()),
            seconds=time.perf_counter() - started,
            inferred_inputs=len(memo),
            pooled_inputs=len(pooled.inputs),
            chunk_inputs=len(structured.units),
            tokens=tokens,
            weighted_tokens=weighted,
            changed_vectors=changed,
        )
        path.write_bytes(orjson.dumps(artifact.model_dump(mode="json")))
        click.echo(
            f"history={captured.index} inputs={len(memo)} "
            f"seconds={artifact.seconds:.2f}",
            err=True,
        )


if __name__ == "__main__":
    main()
