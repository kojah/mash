"""The single registered, prediction-independent annotation enrichment batch."""

import hashlib
import os
from itertools import combinations
from pathlib import Path

import click
import orjson

from blind_turn_pairs import SEED, Manifest, Packet, Pair, Reference, Stratum, View
from corpus import digest
from study_annotations import load_development
from turn_study_corpus import Cohort

ENRICHMENT_SEED = "mash-turn-enrichment-20261005-v1"


def additional_pairs(
    identities: tuple[str, ...], excluded: frozenset[frozenset[str]]
) -> tuple[tuple[int, int], ...]:
    """Four previously unsampled pairs per eligible history; prefer nonadjacent.

    Neither text nor annotations nor model predictions affect this ordering.
    Short histories can contribute fewer pairs, never invented duplicates.
    """
    remaining = (
        pair
        for pair in combinations(range(len(identities)), 2)
        if frozenset((identities[pair[0]], identities[pair[1]])) not in excluded
    )
    return tuple(
        sorted(
            remaining,
            key=lambda p: (
                p[1] - p[0] < 2,
                digest(f"{ENRICHMENT_SEED}:{identities[p[0]]}:{identities[p[1]]}"),
            ),
        )[:4]
    )


def freeze(cohort_path: Path, protocol: Path, root: Path) -> Manifest:
    os.umask(0o077)
    initial_bytes = (root / "manifest.json").read_bytes()
    initial = Manifest.model_validate_json(initial_bytes)
    if initial.initial_manifest_sha256 is not None:
        raise ValueError("the single enrichment batch has already been frozen")
    cohort_bytes = cohort_path.read_bytes()
    if (
        initial.cohort_sha256 != hashlib.sha256(cohort_bytes).hexdigest()
        or initial.protocol_sha256 != hashlib.sha256(protocol.read_bytes()).hexdigest()
    ):
        raise ValueError("enrichment provenance mismatch")
    cohort = Cohort.model_validate_json(cohort_bytes)
    excluded = frozenset(
        frozenset((ref.a_turn_id, ref.b_turn_id)) for ref in initial.references
    )
    pairs: list[Pair] = []
    references: list[Reference] = []
    for entry in cohort.entries:
        turns = tuple(turn for turn in entry.captured.turns if turn.prompts)
        identities = tuple(turn.id for turn in turns)
        for a, b in additional_pairs(identities, excluded):
            gap = b - a
            identity = digest(ENRICHMENT_SEED + identities[a] + identities[b])
            if int(identity[0], 16) % 2:
                a, b = b, a
            views = []
            for index in (a, b):
                opaque = digest(SEED + initial.cohort_sha256 + identities[index])
                path = root / "turns" / f"{opaque}.txt"
                text = path.read_text()
                excerpt = (
                    text
                    if len(text) <= 4000
                    else text[:3000] + "\n[excerpt shortened]\n" + text[-1000:]
                )
                views.append(View(id=opaque, excerpt=excerpt, full_text=path))
            pairs.append(Pair(id=identity, a=views[0], b=views[1]))
            references.append(
                Reference(
                    pair_id=identity,
                    thread_index=entry.captured.index,
                    phase=entry.phase,
                    a_turn_id=identities[a],
                    b_turn_id=identities[b],
                    stratum=Stratum.DISTANT if gap >= 4 else Stratum.FILL,
                    batch=1,
                )
            )
    pairs.sort(key=lambda pair: digest(ENRICHMENT_SEED + pair.id))
    packets = []
    review_packets = []
    for name, collection, offset in (
        ("packet", pairs, len(initial.packets)),
        (
            "review",
            [
                pair
                for pair in pairs
                if int(digest("review" + pair.id)[:8], 16) % 10 == 0
            ],
            len(initial.review_packets),
        ),
    ):
        for index in range(0, len(collection), 24):
            path = root / f"{name}-{offset + index // 24:03}.json"
            with path.open("xb") as file:
                file.write(
                    orjson.dumps(
                        Packet(pairs=tuple(collection[index : index + 24])).model_dump(
                            mode="json"
                        )
                    )
                )
            (packets if name == "packet" else review_packets).append(path)
    enlarged = initial.model_copy(
        update={
            "references": initial.references + tuple(references),
            "packets": initial.packets + tuple(packets),
            "review_packets": initial.review_packets + tuple(review_packets),
            "initial_manifest_sha256": hashlib.sha256(initial_bytes).hexdigest(),
        }
    )
    with (root / "manifest-initial.json").open("xb") as file:
        file.write(initial_bytes)
    pending = root / "manifest-enriched.json"
    with pending.open("xb") as file:
        file.write(orjson.dumps(enlarged.model_dump(mode="json")))
    pending.replace(root / "manifest.json")
    return enlarged


@click.command()
@click.option("--cohort", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--protocol", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--annotations", type=click.Path(path_type=Path, exists=True), required=True
)
def main(cohort: Path, protocol: Path, annotations: Path) -> None:
    labels = load_development(annotations)
    verdicts = tuple(label.verdict.value for label in labels.labels.values())
    if (
        verdicts.count("related") >= 100
        and verdicts.count("unrelated") >= 200
        and labels.confirmation_adequate
    ):
        raise click.ClickException(
            "label adequacy is sufficient; enrichment not needed"
        )
    manifest = freeze(cohort, protocol, annotations)
    click.echo(
        f"Enlarged manifest: {len(manifest.references)} pairs, "
        f"{len(manifest.packets)} primary and "
        f"{len(manifest.review_packets)} review packets"
    )


if __name__ == "__main__":
    main()
