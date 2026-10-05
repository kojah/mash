"""Reconstruct pinned ablation graphs without annotation labels or inference."""

import hashlib
from pathlib import Path

from ablation_evidence import CELLS, fit, fuse
from build_structure_followup import StructuredArtifact
from build_turn_ablation import Artifact
from run_turn_ablation import AblationReport, matrix
from temporal_smoothing import Setting, lineages, smooth
from turn_study_corpus import Entry


def graphs(
    entry: Entry,
    report: AblationReport,
    pooled: Path,
    structural: Path,
    conditioned: Path,
) -> dict[str, dict[str, frozenset[tuple[int, int, int]]]]:
    def read(path: Path, key: str) -> bytes:
        raw = path.read_bytes()
        if hashlib.sha256(raw).hexdigest() != report.report.artifact_sha256[key]:
            raise ValueError("frozen ablation input/vector changed")
        return raw

    index = entry.captured.index
    name = f"{index}-context_whole.npz"
    vectors, owners = matrix(read(pooled / name, f"pooled/{name}"))
    vectors = smooth(
        vectors,
        owners,
        lineages(entry.captured.thread, entry.captured.turns),
        Setting(alpha=0.10, window=4),
    )
    a = fit(vectors, owners)
    results = {}
    for context in (False, True):
        if context:
            name = f"{index}.json"
            built = Artifact.model_validate_json(
                read(conditioned / name, f"conditioned/{name}")
            )
            expected_owners = tuple(u.body.turn_id for u in built.units)
            vectors, point_owners = matrix(
                read(conditioned / f"{index}.npz", f"conditioned/{index}.npz")
            )
        else:
            name = f"{index}-structure_heading.json"
            source = StructuredArtifact.model_validate_json(
                read(structural / name, f"structural/{name}")
            )
            expected_owners = tuple(u.input.turn_id for u in source.units)
            vectors, point_owners = matrix(
                read(
                    structural / f"{index}-structure_heading.npz",
                    f"structural/{index}.npz",
                )
            )
        if point_owners != expected_owners or set(point_owners) != set(owners):
            raise ValueError("frozen point ownership changed")
        b = fit(vectors, point_owners)
        for cell in CELLS:
            if cell.context == context and not cell.generic:
                results[cell.name] = fuse(a, b, (0.0,) * len(b.owners), cell.fusion)
    return results
