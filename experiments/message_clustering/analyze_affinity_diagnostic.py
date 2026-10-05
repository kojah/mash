"""Score complete reranker diagnostics on DEV labels; never promote a method."""

import hashlib
from pathlib import Path

import click
import numpy as np
import orjson

from corpus import Record
from followup_inputs import Variant
from local_affinity import Manifest, sigmoid
from representation_diagnostic import Observation, Summary, summarize
from run_affinity_diagnostic import VIEWS, History
from run_input_followup import FollowupReport
from run_turn_ensemble import A_SHA
from study_annotations import load_development
from turn_study_corpus import Cohort, Phase


class Cost(Record):
    variant: Variant
    scope: str
    turns: int
    truncated: int
    retained_tokens: int
    full_tokens: int
    directional_difference_median: float
    directional_difference_p95: float
    inference_seconds: float
    inference_per_history_p95: float
    measured_directed_pairs: int
    unmeasured_all_directed_pairs: int


class Analysis(Record):
    cohort_sha256: str
    protocol_sha256: str
    parent_sha256: str
    annotation_manifest_sha256: str
    annotation_sha256: dict[str, str]
    history_sha256: dict[str, str]
    model: Manifest
    summaries: tuple[Summary, ...]
    costs: tuple[Cost, ...]
    observations: tuple[Observation, ...]
    confirmation_status: str = "sealed"
    decision: None = None


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--histories", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    parent: Path,
    annotations: Path,
    histories: Path,
    output: Path,
) -> None:
    if output.exists():
        raise click.ClickException("affinity analysis exists; do not overwrite")
    raw = cohort.read_bytes()
    cohort_sha = hashlib.sha256(raw).hexdigest()
    data = Cohort.model_validate_json(raw)
    parent_raw = parent.read_bytes()
    if hashlib.sha256(parent_raw).hexdigest() != A_SHA:
        raise click.ClickException("input parent report changed")
    source = FollowupReport.model_validate_json(parent_raw).report
    labels = load_development(annotations)
    if (
        source.cohort_sha256 != cohort_sha
        or source.annotation_manifest_sha256 != labels.manifest_sha256
        or source.annotation_sha256 != labels.file_sha256
    ):
        raise click.ClickException("cohort or labels differ from frozen parents")
    protocol_sha = hashlib.sha256(protocol.read_bytes()).hexdigest()
    hashes = {}
    saved_histories = []
    observations = []
    for entry in data.entries:
        if entry.phase != Phase.DEVELOPMENT:
            continue
        index = entry.captured.index
        name = f"{index}.json"
        history_raw = (histories / name).read_bytes()
        history = History.model_validate_json(history_raw)
        hashes[name] = hashlib.sha256(history_raw).hexdigest()
        turns = tuple(t.id for t in entry.captured.turns if t.prompts)
        positions = {owner: i for i, owner in enumerate(turns)}
        refs = tuple(
            r
            for r in labels.manifest.references
            if r.thread_index == index and r.phase == Phase.DEVELOPMENT
        )
        if (
            history.thread_index != index
            or history.cohort_sha256 != cohort_sha
            or history.protocol_sha256 != protocol_sha
            or history.annotation_manifest_sha256 != labels.manifest_sha256
            or tuple(v.variant for v in history.views) != VIEWS
            or history.all_pairs_directed_per_view != len(turns) * (len(turns) - 1)
        ):
            raise click.ClickException("history scope/provenance changed")
        if saved_histories and history.model != saved_histories[0].model:
            raise click.ClickException("model runtime differs across histories")
        saved_histories.append(history)
        for view in history.views:
            if (
                view.input_sha256
                != source.artifact_sha256[f"{index}-{view.variant}.json"]
                or tuple(t.turn_id for t in view.turns) != turns
                or tuple(s.pair_id for s in view.scores)
                != tuple(r.pair_id for r in refs)
            ):
                raise click.ClickException("view inputs or pair identities changed")
            for ref, score in zip(refs, view.scores, strict=True):
                if (
                    score.forward != sigmoid(score.forward_logit)
                    or score.reverse != sigmoid(score.reverse_logit)
                    or score.mean != (score.forward + score.reverse) / 2
                ):
                    raise click.ClickException("stored affinity reducers differ")
                observations.append(
                    Observation(
                        view=view.variant.value,
                        thread_index=index,
                        pair_id=ref.pair_id,
                        verdict=labels.labels[ref.pair_id].verdict,
                        turns=len(turns),
                        distance=abs(
                            positions[ref.a_turn_id] - positions[ref.b_turn_id]
                        ),
                        similarity=score.mean,
                        comparisons=2,
                    )
                )
    frozen = tuple(observations)
    costs = []
    for variant in VIEWS:
        for scope in ("all", "short", "long"):
            chosen = tuple(
                h
                for h in saved_histories
                if scope == "all"
                or ("long" if len(h.views[0].turns) >= 8 else "short") == scope
            )
            views = tuple(
                next(v for v in h.views if v.variant == variant) for h in chosen
            )
            diffs = [abs(s.forward - s.reverse) for v in views for s in v.scores]
            costs.append(
                Cost(
                    variant=variant,
                    scope=scope,
                    turns=sum(len(v.turns) for v in views),
                    truncated=sum(t.truncated for v in views for t in v.turns),
                    retained_tokens=sum(
                        t.retained_tokens for v in views for t in v.turns
                    ),
                    full_tokens=sum(t.full_tokens for v in views for t in v.turns),
                    directional_difference_median=float(np.median(diffs)),
                    directional_difference_p95=float(np.quantile(diffs, 0.95)),
                    inference_seconds=sum(v.inference_seconds for v in views),
                    inference_per_history_p95=float(
                        np.quantile([v.inference_seconds for v in views], 0.95)
                    ),
                    measured_directed_pairs=2 * sum(len(v.scores) for v in views),
                    unmeasured_all_directed_pairs=sum(
                        h.all_pairs_directed_per_view for h in chosen
                    ),
                )
            )
    result = Analysis(
        cohort_sha256=cohort_sha,
        protocol_sha256=protocol_sha,
        parent_sha256=A_SHA,
        annotation_manifest_sha256=labels.manifest_sha256,
        annotation_sha256=labels.file_sha256,
        history_sha256=hashes,
        model=saved_histories[0].model,
        observations=frozen,
        costs=tuple(costs),
        summaries=tuple(
            summarize(frozen, variant.value, scope)
            for variant in VIEWS
            for scope in ("all", "short", "long", "nearby", "distant")
        ),
    )
    with output.open("xb") as stream:
        stream.write(
            orjson.dumps(result.model_dump(mode="json"), option=orjson.OPT_INDENT_2)
        )
    click.echo("Affinity diagnostic scored on DEV only; no method accepted")


if __name__ == "__main__":
    main()
