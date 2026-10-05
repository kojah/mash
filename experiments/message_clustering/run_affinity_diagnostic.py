"""Infer frozen DEV reference pairs without reading any annotation verdicts."""

import hashlib
import os
import time
from pathlib import Path

import click
import orjson

from analyze_input_followup import validate_sources
from blind_turn_pairs import Manifest as PairManifest
from build_input_followup import Built
from corpus import Record
from followup_inputs import Variant
from local_affinity import Affinity, Manifest, Prefix, prefix
from run_input_followup import FollowupReport
from run_turn_ensemble import A_SHA
from turn_study_corpus import Cohort, Phase

VIEWS = (Variant.LEAN, Variant.CONTEXT_MEAN)


class PairScore(Record):
    pair_id: str
    forward_logit: float
    reverse_logit: float
    forward: float
    reverse: float
    mean: float


class InputCost(Record):
    turn_id: str
    source_sha256: str
    full_tokens: int
    retained_tokens: int
    truncated: bool


class ViewScores(Record):
    variant: Variant
    input_sha256: str
    turns: tuple[InputCost, ...]
    scores: tuple[PairScore, ...]
    preparation_seconds: float
    inference_seconds: float


class History(Record):
    thread_index: int
    protocol_sha256: str
    cohort_sha256: str
    annotation_manifest_sha256: str
    model: Manifest
    views: tuple[ViewScores, ...]
    all_pairs_directed_per_view: int


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--manifest", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    manifest: Path,
    parent: Path,
    artifacts: Path,
    model: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    raw = cohort.read_bytes()
    cohort_sha = hashlib.sha256(raw).hexdigest()
    captured = Cohort.model_validate_json(raw)
    parent_raw = parent.read_bytes()
    if hashlib.sha256(parent_raw).hexdigest() != A_SHA:
        raise click.ClickException("frozen input parent changed")
    source = FollowupReport.model_validate_json(parent_raw).report
    manifest_raw = manifest.read_bytes()
    pairs = PairManifest.model_validate_json(manifest_raw)
    manifest_sha = hashlib.sha256(manifest_raw).hexdigest()
    if (
        source.cohort_sha256 != cohort_sha
        or pairs.cohort_sha256 != cohort_sha
        or source.annotation_manifest_sha256 != manifest_sha
    ):
        raise click.ClickException("cohort/pair manifest provenance differs")
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    affinity = Affinity(model)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    for entry in captured.entries:
        if entry.phase != Phase.DEVELOPMENT:
            continue
        index = entry.captured.index
        turns = tuple(t.id for t in entry.captured.turns if t.prompts)
        refs = tuple(
            r
            for r in pairs.references
            if r.phase == Phase.DEVELOPMENT and r.thread_index == index
        )
        destination = output / f"{index}.json"
        if destination.exists():
            saved = History.model_validate_json(destination.read_bytes())
            if (
                saved.thread_index != index
                or saved.protocol_sha256 != protocol_sha
                or saved.cohort_sha256 != cohort_sha
                or saved.annotation_manifest_sha256 != manifest_sha
                or saved.model != affinity.manifest
                or tuple(v.variant for v in saved.views) != VIEWS
                or any(
                    tuple(s.pair_id for s in v.scores) != tuple(r.pair_id for r in refs)
                    or v.input_sha256
                    != source.artifact_sha256[f"{index}-{v.variant}.json"]
                    for v in saved.views
                )
            ):
                raise click.ClickException("resume metadata differs from frozen inputs")
            click.echo(f"history {index}: validated resume")
            continue
        views = []
        for variant in VIEWS:
            started = time.perf_counter()
            data = (artifacts / f"{index}-{variant}.json").read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            if digest != source.artifact_sha256[f"{index}-{variant}.json"]:
                raise click.ClickException("input provenance changed")
            built = Built.model_validate_json(data)
            if (
                built.thread_index != index
                or built.phase != Phase.DEVELOPMENT
                or built.variant != variant
                or built.turn_ids != turns
            ):
                raise click.ClickException("input turn identities changed")
            validate_sources(built, entry.captured.thread, entry.captured.turns)
            inputs: dict[str, Prefix] = {}
            costs = []
            for turn in turns:
                text = "\n\n".join(
                    i.content.text for i in built.inputs if i.turn_id == turn
                )
                inputs[turn] = prefix(text, affinity.tokenizer)
                costs.append(
                    InputCost(
                        turn_id=turn,
                        source_sha256=hashlib.sha256(text.encode()).hexdigest(),
                        full_tokens=inputs[turn].full_tokens,
                        retained_tokens=inputs[turn].retained_tokens,
                        truncated=inputs[turn].truncated,
                    )
                )
            preparation = time.perf_counter() - started
            started = time.perf_counter()
            scores = []
            for ref in refs:
                fl, forward = affinity.score(
                    inputs[ref.a_turn_id], inputs[ref.b_turn_id]
                )
                rl, reverse = affinity.score(
                    inputs[ref.b_turn_id], inputs[ref.a_turn_id]
                )
                scores.append(
                    PairScore(
                        pair_id=ref.pair_id,
                        forward_logit=fl,
                        reverse_logit=rl,
                        forward=forward,
                        reverse=reverse,
                        mean=(forward + reverse) / 2,
                    )
                )
            views.append(
                ViewScores(
                    variant=variant,
                    input_sha256=digest,
                    turns=tuple(costs),
                    scores=tuple(scores),
                    preparation_seconds=preparation,
                    inference_seconds=time.perf_counter() - started,
                )
            )
        result = History(
            thread_index=index,
            protocol_sha256=protocol_sha,
            cohort_sha256=cohort_sha,
            annotation_manifest_sha256=manifest_sha,
            model=affinity.manifest,
            views=tuple(views),
            all_pairs_directed_per_view=len(turns) * (len(turns) - 1),
        )
        temporary = destination.with_suffix(".partial")
        with temporary.open("wb") as stream:
            stream.write(
                orjson.dumps(result.model_dump(mode="json"), option=orjson.OPT_INDENT_2)
            )
        temporary.rename(destination)
        click.echo(f"history {index}: {len(refs)} pairs, two inventories complete")


if __name__ == "__main__":
    main()
