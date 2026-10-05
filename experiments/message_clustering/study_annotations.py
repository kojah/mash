"""Validated blind judgments with a development-only view and sealed adequacy check."""

import hashlib
from dataclasses import dataclass
from pathlib import Path

from blind_turn_pairs import Annotations, Label, Manifest, Packet, Verdict, validate
from corpus import Record
from turn_study_corpus import Phase


class Agreement(Record):
    pairs: int
    agrees: int
    decisive_pairs: int
    decisive_agrees: int
    conflicts: tuple[str, ...]


@dataclass(frozen=True)
class DevelopmentLabels:
    manifest: Manifest
    manifest_sha256: str
    labels: dict[str, Label]
    file_sha256: dict[str, str]
    confirmation_adequate: bool
    agreement: Agreement


def load_development(root: Path) -> DevelopmentLabels:
    """Validate all packets; expose confirmation only as a preregistered boolean.

    Review labels never replace primary labels. Their agreement is diagnostic,
    and confirmation disagreements remain hidden until a choice is frozen.
    """
    manifest_bytes = (root / "manifest.json").read_bytes()
    manifest = Manifest.model_validate_json(manifest_bytes)
    primary: dict[str, Label] = {}
    review: dict[str, Label] = {}
    checksums: dict[str, str] = {}
    for packets, destination in (
        (manifest.packets, primary),
        (manifest.review_packets, review),
    ):
        for path in packets:
            label_path = root / ("labels-" + path.name)
            raw = label_path.read_bytes()
            annotations = Annotations.model_validate_json(raw)
            validate(Packet.model_validate_json(path.read_bytes()), annotations)
            checksums[label_path.name] = hashlib.sha256(raw).hexdigest()
            for label in annotations.labels:
                if label.pair_id in destination:
                    raise ValueError("pair ID occurs in more than one packet")
                destination[label.pair_id] = label
    refs = {ref.pair_id: ref for ref in manifest.references}
    if len(refs) != len(manifest.references) or set(primary) != set(refs):
        raise ValueError("primary annotations must cover the frozen manifest exactly")
    if not set(review) <= set(primary):
        raise ValueError("review pair is absent from the primary manifest")
    confirmation = tuple(
        label.verdict
        for identity, label in primary.items()
        if refs[identity].phase == Phase.CONFIRMATION
    )
    labels = {
        identity: label
        for identity, label in primary.items()
        if refs[identity].phase == Phase.DEVELOPMENT
    }
    reviewed = tuple(identity for identity in review if identity in labels)
    decisive = tuple(
        identity
        for identity in reviewed
        if Verdict.AMBIGUOUS not in (labels[identity].verdict, review[identity].verdict)
    )
    conflicts = tuple(
        identity
        for identity in reviewed
        if labels[identity].verdict != review[identity].verdict
    )
    return DevelopmentLabels(
        manifest=manifest,
        manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
        labels=labels,
        file_sha256=checksums,
        confirmation_adequate=(
            confirmation.count(Verdict.RELATED) >= 20
            and confirmation.count(Verdict.UNRELATED) >= 40
        ),
        agreement=Agreement(
            pairs=len(reviewed),
            agrees=len(reviewed) - len(conflicts),
            decisive_pairs=len(decisive),
            decisive_agrees=sum(
                labels[i].verdict == review[i].verdict for i in decisive
            ),
            conflicts=conflicts,
        ),
    )
