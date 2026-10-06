"""Full-population baseline diagnostics, with no inference or parameter tuning."""

import os
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from ablation_evidence import fit, fuse
from blind_turn_pairs import Verdict
from build_input_followup import Built
from build_local_representation import COHORT_SHA, pinned, source
from build_structure_followup import own_mask
from build_token_weighting import digest
from corpus import Record, Thread
from followup_inputs import Input, lean_prompt
from freeze_link_review import SOURCE_SHA
from run_turn_ablation import AblationReport, matrix
from study_annotations import load_development
from temporal_smoothing import Setting, lineages, smooth
from turn_study_corpus import Cohort, Phase
from turns import Turn


class Observation(Record):
    pair_id: str
    thread_index: int
    provider: str
    node: str
    long: bool
    verdict: Verdict
    stratum: str
    batch: int
    distance: int
    short_prompt: bool
    evidence_a_covered: bool | None
    evidence_b_covered: bool | None
    full: bool
    prefix: bool

    @property
    def error(self) -> bool:
        return self.full != (self.verdict == Verdict.RELATED)


class Slice(Record):
    dimension: str
    value: str
    pairs: int
    errors: int
    false_positives: int
    false_negatives: int


class Prefix(Record):
    pairs_with_later_turns: int
    changed: int
    errors_corrected: int
    successes_broken: int
    corrected_false_positives: int
    corrected_false_negatives: int


class Audit(Record):
    baseline_sha256: str = SOURCE_SHA
    cohort_sha256: str = COHORT_SHA
    annotation_sha256: dict[str, str]
    observations: tuple[Observation, ...]
    slices: tuple[Slice, ...]
    prefix: Prefix
    source_inputs_sha256: dict[str, str]


def evidence_covered(
    thread: Thread, turn: Turn, quote: str, inputs: tuple[Input, ...]
) -> bool | None:
    """Locate literal quotes in original sources; unknown isn't treated as omitted."""
    text_parts = []
    ranges = []
    offset = 0
    for position, index in enumerate(turn.sources):
        text = thread.messages[index].projection
        separator = "\n\n" if position else ""
        header = f"{thread.messages[index].role.value.upper()}:\n"
        start = offset + len(separator) + len(header)
        ranges.append((index, start, start + len(text)))
        text_parts.append(separator + header + text)
        offset = start + len(text)
    rendered = "".join(text_parts)
    start = rendered.find(quote)
    located = False
    while quote and start >= 0:
        covered = True
        content = False
        for index, left, right in ranges:
            begin, end = max(start, left), min(start + len(quote), right)
            if begin >= end:
                continue
            text = thread.messages[index].projection
            mask = own_mask(inputs, turn.id, index, len(text))
            required = np.fromiter(
                (not c.isspace() for c in rendered[begin:end]), dtype=np.bool_
            )
            content |= bool(required.any())
            located |= bool(required.any())
            covered &= bool(np.all(mask[begin - left : end - left][required]))
        if content and covered:
            return True
        start = rendered.find(quote, start + 1)
    return False if located else None


def prefix_members(
    a: np.ndarray,
    a_owners: tuple[str, ...],
    b: np.ndarray,
    b_owners: tuple[str, ...],
    stop: int,
):
    retained = set(a_owners[:stop])
    mask = np.array([o in retained for o in b_owners], dtype=np.bool_)
    selected = tuple(o for o in b_owners if o in retained)
    return fuse(
        fit(a[:stop], a_owners[:stop]),
        fit(b[mask], selected),
        (0.0,) * len(selected),
        False,
    )


def summarize(observations: tuple[Observation, ...]) -> tuple[Slice, ...]:
    buckets: dict[tuple[str, str], list[Observation]] = {}
    for o in observations:
        for dimension, value in (
            ("all", "all"),
            ("length", "8+" if o.long else "2-7"),
            ("truth", o.verdict.value),
            ("stratum", o.stratum),
            ("provider", o.provider),
            ("node", o.node),
            ("distance", "1" if o.distance == 1 else "2-3" if o.distance < 4 else "4+"),
            ("short_prompt_proxy", str(o.short_prompt)),
            (
                "omitted_quote",
                str(False in (o.evidence_a_covered, o.evidence_b_covered)),
            ),
            (
                "unlocatable_quote",
                str(None in (o.evidence_a_covered, o.evidence_b_covered)),
            ),
            ("length_truth", f"{'8+' if o.long else '2-7'}/{o.verdict.value}"),
        ):
            buckets.setdefault((dimension, value), []).append(o)
    return tuple(
        Slice(
            dimension=d,
            value=v,
            pairs=len(rows),
            errors=sum(o.error for o in rows),
            false_positives=sum(o.error and o.full for o in rows),
            false_negatives=sum(o.error and not o.full for o in rows),
        )
        for (d, v), rows in sorted(buckets.items())
    )


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(parent: Path, output: Path) -> None:
    os.umask(0o077)
    raw = (parent / "turn-three-factor-ablation-20261006/development.json").read_bytes()
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    if digest(raw) != SOURCE_SHA or digest(cohort_raw) != COHORT_SHA:
        raise ValueError("frozen inputs changed")
    report = AblationReport.model_validate_json(raw)
    cohort = Cohort.model_validate_json(cohort_raw)
    labels = load_development(parent / "turn-study-blind-20261005")
    if labels.file_sha256 != report.report.annotation_sha256:
        raise ValueError("annotations changed")
    predictions = {
        p.pair_id: p.overlapping
        for r in report.report.rows
        if r.name == "c0f0g0"
        for p in r.predictions
    }
    observations = []
    input_hashes = {}
    later = changed = corrected = broken = corrected_fp = corrected_fn = 0
    with threadpool_limits(limits=1):
        for entry in cohort.entries:
            if entry.phase != Phase.DEVELOPMENT:
                continue
            captured = entry.captured
            turns = tuple(t for t in captured.turns if t.prompts)
            turn_by_id = {t.id: t for t in turns}
            structured, b, _, _ = source(entry, parent, report)
            pooled_path = (
                parent
                / "turn-input-followup-20261005/artifacts"
                / f"{captured.index}-context_whole.json"
            )
            pooled_raw = pooled_path.read_bytes()
            if digest(pooled_raw) != structured.source_input_sha256:
                raise ValueError("source input provenance differs")
            pooled = Built.model_validate_json(pooled_raw)
            input_hashes[pooled_path.name] = digest(pooled_raw)
            inputs = pooled.inputs + tuple(u.input for u in structured.units)
            filename = f"{captured.index}-context_whole.npz"
            a, a_owners = matrix(
                pinned(pooled_path.with_suffix(".npz"), f"pooled/{filename}", report)
            )
            a = smooth(
                a,
                a_owners,
                lineages(captured.thread, captured.turns),
                Setting(alpha=0.10, window=4),
            )
            b_owners = tuple(u.input.turn_id for u in structured.units)
            full = prefix_members(a, a_owners, b, b_owners, len(a_owners))
            cache = {len(a_owners): full}
            for ref in labels.manifest.references:
                if (
                    ref.thread_index != captured.index
                    or ref.pair_id not in labels.labels
                ):
                    continue
                label = labels.labels[ref.pair_id]
                if label.verdict == Verdict.AMBIGUOUS:
                    continue
                prediction = bool(full[ref.a_turn_id] & full[ref.b_turn_id])
                if prediction != predictions[ref.pair_id]:
                    raise ValueError("baseline membership reproduction failed")
                positions = (
                    a_owners.index(ref.a_turn_id),
                    a_owners.index(ref.b_turn_id),
                )
                stop = max(positions) + 1
                if stop not in cache:
                    cache[stop] = prefix_members(a, a_owners, b, b_owners, stop)
                prefix = bool(cache[stop][ref.a_turn_id] & cache[stop][ref.b_turn_id])
                truth = label.verdict == Verdict.RELATED
                if stop < len(a_owners):
                    later += 1
                    changed += prefix != prediction
                    corrected += prediction != truth and prefix == truth
                    broken += prediction == truth and prefix != truth
                    corrected_fp += prediction and not prefix and not truth
                    corrected_fn += not prediction and prefix and truth
                pair_turns = (turn_by_id[ref.a_turn_id], turn_by_id[ref.b_turn_id])
                short = any(
                    len(
                        " ".join(
                            lean_prompt(captured.thread, i).text for i in t.prompts
                        ).split()
                    )
                    <= 12
                    for t in pair_turns
                )
                observations.append(
                    Observation(
                        pair_id=ref.pair_id,
                        thread_index=captured.index,
                        provider=captured.thread.provider.value,
                        node=entry.node,
                        long=len(turns) >= 8,
                        verdict=label.verdict,
                        stratum=ref.stratum.value,
                        batch=ref.batch,
                        distance=abs(positions[0] - positions[1]),
                        short_prompt=short,
                        evidence_a_covered=evidence_covered(
                            captured.thread, pair_turns[0], label.evidence_a, inputs
                        ),
                        evidence_b_covered=evidence_covered(
                            captured.thread, pair_turns[1], label.evidence_b, inputs
                        ),
                        full=prediction,
                        prefix=prefix,
                    )
                )
    audit = Audit(
        annotation_sha256=labels.file_sha256,
        observations=tuple(observations),
        slices=summarize(tuple(observations)),
        source_inputs_sha256=input_hashes,
        prefix=Prefix(
            pairs_with_later_turns=later,
            changed=changed,
            errors_corrected=corrected,
            successes_broken=broken,
            corrected_false_positives=corrected_fp,
            corrected_false_negatives=corrected_fn,
        ),
    )
    output.write_bytes(orjson.dumps(audit.model_dump(mode="json")))
    click.echo(
        f"Audited {len(observations)} decisive pairs; "
        "reproduced every baseline prediction"
    )


if __name__ == "__main__":
    main()
