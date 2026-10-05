"""Freeze blinded changed-link packets without reading annotation verdicts."""

import hashlib
import os
from collections import Counter
from itertools import combinations
from pathlib import Path

import click
import orjson
from threadpoolctl import threadpool_limits

from ablation_evidence import CELLS, Cell
from blind_turn_pairs import Annotations, Manifest, Packet, Pair, View
from corpus import Record, digest
from frozen_ablation_graph import graphs
from link_review import SEED, Candidate, Draw, select
from run_turn_ablation import AblationReport
from turn_study_corpus import Cohort, Phase

SOURCE_SHA = "19ae4fd9f2c25b243416ac0a27fd735f3e286d9e5115be68fcb1830b712fd099"
ADMISSIBLE = tuple(c for c in CELLS if not c.generic)


class Prediction(Record):
    cell: Cell
    matched: bool


class Reference(Record):
    draw: Draw
    thread_index: int
    a_turn_id: str
    b_turn_id: str
    distance: int
    long: bool
    predictions: tuple[Prediction, ...]


class ReviewManifest(Record):
    seed: str = SEED
    cohort_sha256: str
    protocol_sha256: str
    ablation_sha256: str = SOURCE_SHA
    original_manifest_sha256: str
    references: tuple[Reference, ...]
    primary_packets: tuple[Path, ...]
    review_packets: tuple[Path, ...]
    file_sha256: dict[str, str]
    pool_counts: dict[str, int]


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--ablation", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--original-manifest", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--pooled", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--structural", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option(
    "--conditioned", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    cohort: Path,
    protocol: Path,
    ablation: Path,
    original_manifest: Path,
    pooled: Path,
    structural: Path,
    conditioned: Path,
    output: Path,
) -> None:
    os.umask(0o077)
    raw = ablation.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SOURCE_SHA:
        raise click.ClickException("frozen ablation report changed")
    report = AblationReport.model_validate_json(raw)
    cohort_raw = cohort.read_bytes()
    captured = Cohort.model_validate_json(cohort_raw)
    original_raw = original_manifest.read_bytes()
    original = Manifest.model_validate_json(original_raw)
    if (
        hashlib.sha256(cohort_raw).hexdigest() != report.report.cohort_sha256
        or original.cohort_sha256 != report.report.cohort_sha256
        or hashlib.sha256(original_raw).hexdigest()
        != report.report.annotation_manifest_sha256
    ):
        raise click.ClickException("cohort or original reference identities changed")
    excluded = {
        (r.thread_index, *sorted((r.a_turn_id, r.b_turn_id)))
        for r in original.references
    }
    entries = {
        e.captured.index: e for e in captured.entries if e.phase == Phase.DEVELOPMENT
    }
    changed = []
    stable = []
    pools: Counter[str] = Counter()
    with threadpool_limits(limits=1):
        for index, entry in entries.items():
            members = graphs(entry, report, pooled, structural, conditioned)
            owners = tuple(t.id for t in entry.captured.turns if t.prompts)
            # Verify graph reproduction before constructing any selection pools.
            prior_refs = {
                r.pair_id: r for r in original.references if r.thread_index == index
            }
            for row in report.report.rows:
                if row.thread_index != index or row.name not in members:
                    continue
                for prediction in row.predictions:
                    ref = prior_refs[prediction.pair_id]
                    if (
                        bool(
                            members[row.name][ref.a_turn_id]
                            & members[row.name][ref.b_turn_id]
                        )
                        != prediction.overlapping
                    ):
                        raise click.ClickException(
                            "full graph differs from frozen predictions"
                        )
            for i, j in combinations(range(len(owners)), 2):
                a, b = owners[i], owners[j]
                if (index, *sorted((a, b))) in excluded:
                    continue
                identity = digest(
                    f"{SEED}:{report.report.cohort_sha256}:{index}:{a}:{b}"
                )
                predictions = tuple(
                    bool(members[c.name][a] & members[c.name][b]) for c in ADMISSIBLE
                )
                case = Candidate(
                    identity,
                    index,
                    a,
                    b,
                    j - i,
                    len(owners) >= 8,
                    entry.captured.thread.provider.value,
                    entry.node,
                    predictions,
                )
                (stable if case.stable else changed).append(case)
                if predictions[2] != predictions[0]:
                    direction = "add" if predictions[2] else "remove"
                    length = "long" if case.long else "short"
                    pools[f"context_{direction}_{length}"] += 1
                if predictions[1] != predictions[0]:
                    pools[f"fusion_{'long' if case.long else 'short'}"] += 1
    draws = select(tuple(changed), tuple(stable))
    by_id = {c.id: c for c in (*changed, *stable)}
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    blind = output / "blind"
    blind.mkdir(mode=0o700)
    texts = blind / "texts"
    texts.mkdir(mode=0o700)
    views = {}
    references = []
    pairs = []
    hashes = {}
    for draw in draws:
        case = by_id[draw.pair_id]
        entry = entries[case.thread_index]
        turn_map = {t.id: t for t in entry.captured.turns if t.prompts}
        for identity in (case.a, case.b):
            if identity in views:
                continue
            opaque = digest(SEED + report.report.cohort_sha256 + identity)
            text = "\n\n".join(
                f"{entry.captured.thread.messages[i].role.value.upper()}:\n{entry.captured.thread.messages[i].projection}"
                for i in turn_map[identity].sources
            )
            path = texts / f"{opaque}.txt"
            path.write_text(text)
            hashes[str(path.relative_to(output))] = hashlib.sha256(
                path.read_bytes()
            ).hexdigest()
            excerpt = (
                text
                if len(text) <= 4000
                else text[:3000] + "\n[excerpt shortened]\n" + text[-1000:]
            )
            views[identity] = View(id=opaque, excerpt=excerpt, full_text=path)
        a, b = (
            (case.a, case.b)
            if int(digest(SEED + case.id)[0], 16) % 2
            else (case.b, case.a)
        )
        pairs.append(Pair(id=case.id, a=views[a], b=views[b]))
        references.append(
            Reference(
                draw=draw,
                thread_index=case.thread_index,
                a_turn_id=a,
                b_turn_id=b,
                distance=case.distance,
                long=case.long,
                predictions=tuple(
                    Prediction(cell=c, matched=v)
                    for c, v in zip(ADMISSIBLE, case.predictions, strict=True)
                ),
            )
        )
    pairs.sort(key=lambda p: digest(SEED + "packet" + p.id))
    reviewing = sorted(pairs, key=lambda p: digest(SEED + "review" + p.id))[:16]
    primary_paths = []
    review_paths = []
    for label, collection, paths in (
        ("packet", pairs, primary_paths),
        ("review", reviewing, review_paths),
    ):
        for start in range(0, len(collection), 12):
            path = blind / f"{label}-{start // 12:03}.json"
            path.write_bytes(
                orjson.dumps(
                    Packet(pairs=tuple(collection[start : start + 12])).model_dump(
                        mode="json"
                    )
                )
            )
            paths.append(path)
            hashes[str(path.relative_to(output))] = hashlib.sha256(
                path.read_bytes()
            ).hexdigest()
    (blind / "annotation.schema.json").write_bytes(
        orjson.dumps(Annotations.model_json_schema())
    )
    manifest = ReviewManifest(
        cohort_sha256=report.report.cohort_sha256,
        protocol_sha256=hashlib.sha256(protocol.read_bytes()).hexdigest(),
        original_manifest_sha256=hashlib.sha256(original_raw).hexdigest(),
        references=tuple(references),
        primary_packets=tuple(primary_paths),
        review_packets=tuple(review_paths),
        file_sha256=hashes,
        pool_counts=dict(pools),
    )
    (output / "manifest.json").write_bytes(
        orjson.dumps(manifest.model_dump(mode="json"))
    )
    click.echo(
        f"Frozen {len(draws)} new pairs; {len(reviewing)} independently reviewed"
    )


if __name__ == "__main__":
    main()
