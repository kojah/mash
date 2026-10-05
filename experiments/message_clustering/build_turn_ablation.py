"""Validate every DEV input before local inference; resume per-history artifacts."""

import hashlib
import os
import time
from collections import Counter
from pathlib import Path

import click
import numpy as np
import orjson

from ablation_evidence import Background, turn_terms
from ablation_inputs import Conditioned, prepare
from build_input_followup import import_cache
from build_structure_followup import StructuredArtifact
from corpus import Record
from local_encoder import Encoder, EncoderManifest
from run_structure_followup import StructureReport
from run_turn_ensemble import B_SHA
from turn_artifact import SelectedText, Store, generation
from turn_study_corpus import Cohort, Entry, Phase


class Artifact(Record):
    thread_index: int
    protocol_sha256: str
    cohort_sha256: str
    encoder_generation: str
    source_sha256: str
    matrix_sha256: str
    units: tuple[Conditioned, ...]
    seconds: float
    new_vectors: int


def inputs(
    entry: Entry, directory: Path, source: StructureReport, encoder: Encoder
) -> tuple[tuple[Conditioned, ...], str]:
    name = f"{entry.captured.index}-structure_heading.json"
    raw = (directory / name).read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if source.report.artifact_sha256[name] != digest:
        raise ValueError("structural input hash differs from frozen parent")
    built = StructuredArtifact.model_validate_json(raw)
    owners = tuple(t.id for t in entry.captured.turns if t.prompts)
    if (
        built.thread_index != entry.captured.index
        or built.phase != Phase.DEVELOPMENT
        or built.turn_ids != owners
    ):
        raise ValueError("structural input identity/phase differs")
    units = prepare(
        built.units, entry.captured.thread, entry.captured.turns, encoder.tokenizer
    )
    return units, digest


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--source-report", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--source-artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    source_report: Path,
    source_artifacts: Path,
    model: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    raw = cohort.read_bytes()
    cohort_sha = hashlib.sha256(raw).hexdigest()
    captured = Cohort.model_validate_json(raw)
    source_raw = source_report.read_bytes()
    if hashlib.sha256(source_raw).hexdigest() != B_SHA:
        raise click.ClickException("structural parent report changed")
    source = StructureReport.model_validate_json(source_raw)
    if source.report.cohort_sha256 != cohort_sha:
        raise click.ClickException("cohort differs from structural parent")
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    encoder = Encoder(model)
    if encoder.manifest != EncoderManifest.model_validate_json(
        (source_artifacts / "encoder.json").read_bytes()
    ):
        raise click.ClickException("installed encoder differs from frozen parent")
    entries = tuple(e for e in captured.entries if e.phase == Phase.DEVELOPMENT)
    counts: Counter[str] = Counter()
    documents = 0
    for entry in entries:
        for turn in entry.captured.turns:
            if turn.prompts:
                counts.update(turn_terms(entry.captured.thread, turn))
                documents += 1
    background = Background(
        documents=documents, document_frequency=dict(sorted(counts.items()))
    )
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    background_raw = orjson.dumps(background.model_dump(mode="json"))
    background_path = output / "background.json"
    if background_path.exists():
        if background_path.read_bytes() != background_raw:
            raise click.ClickException("background vocabulary differs on resume")
    else:
        with background_path.open("xb") as stream:
            stream.write(background_raw)
    fingerprint = generation(encoder)
    # Fail on coverage/provenance before spending time on any new inference.
    for entry in entries:
        inputs(entry, source_artifacts, source, encoder)
    click.echo(f"Preflight passed for {len(entries)} development histories")
    store = Store(output / "vectors.sqlite")
    try:
        import_cache(store, source_artifacts / "vectors.sqlite")
        for entry in entries:
            started = time.perf_counter()
            index = entry.captured.index
            units, source_sha = inputs(entry, source_artifacts, source, encoder)
            destination = output / f"{index}.json"
            matrix_path = output / f"{index}.npz"
            if destination.exists():
                saved = Artifact.model_validate_json(destination.read_bytes())
                if (
                    saved.thread_index != index
                    or saved.protocol_sha256 != protocol_sha
                    or saved.cohort_sha256 != cohort_sha
                    or saved.encoder_generation != fingerprint
                    or saved.source_sha256 != source_sha
                    or saved.units != units
                    or saved.matrix_sha256
                    != hashlib.sha256(matrix_path.read_bytes()).hexdigest()
                ):
                    raise click.ClickException("conditioned artifact differs on resume")
                click.echo(f"history {index}: validated resume")
                continue
            vectors, new = store.selected_vectors(
                tuple(
                    SelectedText(text=u.content.text, selection=u.selection)
                    for u in units
                ),
                encoder,
                lambda done, total: None,
            )
            temporary = matrix_path.with_suffix(".npz.partial")
            with temporary.open("wb") as stream:
                np.savez_compressed(
                    stream,
                    vectors=vectors,
                    turn_ids=np.asarray([u.body.turn_id for u in units]),
                )
            temporary.rename(matrix_path)
            result = Artifact(
                thread_index=index,
                protocol_sha256=protocol_sha,
                cohort_sha256=cohort_sha,
                encoder_generation=fingerprint,
                source_sha256=source_sha,
                matrix_sha256=hashlib.sha256(matrix_path.read_bytes()).hexdigest(),
                units=units,
                seconds=time.perf_counter() - started,
                new_vectors=new,
            )
            pending = destination.with_suffix(".partial")
            with pending.open("wb") as stream:
                stream.write(orjson.dumps(result.model_dump(mode="json")))
            pending.rename(destination)
            click.echo(f"history {index}: {len(units)} points, {new} new vectors")
    finally:
        store.close()


if __name__ == "__main__":
    main()
