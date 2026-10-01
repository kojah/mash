"""Deterministic, model-output-free development sampling for source review."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import sqlite3
import re
import tomllib

from data import load
from episodes import read_turns, source_hash


class CohortProvider(StrEnum):
    CLAUDE = "claude-code"
    CODEX = "codex"


class LengthStratum(StrEnum):
    SHORT = "short"
    MEDIUM = "medium"


@dataclass(frozen=True)
class Candidate:
    thread: str
    provider: CohortProvider
    group: bytes
    stratum: LengthStratum


def review_text(text: str) -> tuple[str, bool]:
    """Conservative display guard, not a general secret scanner or source filter."""
    sensitive = re.fullmatch(r"[A-Za-z0-9_#=+/-]{40,}", text.strip()) is not None
    return ("[redacted credential-like standalone value]", True) if sensitive else (text, False)


def select(candidates: list[Candidate], seed: str, per_stratum: int = 2) -> tuple[Candidate, ...]:
    if not isinstance(seed, str) or not seed.strip() or type(per_stratum) is not int or not 1 <= per_stratum <= 16:
        raise ValueError("nonempty sampling seed and bounded positive stratum count required")
    if any(len(c.group) != 32 or c.group[0] % 5 == 0 for c in candidates) or len({c.group for c in candidates}) != len(candidates):
        raise ValueError("sampling must use independent development source groups")
    strata = sorted({(c.provider, c.stratum) for c in candidates})
    if strata != [(CohortProvider.CLAUDE, LengthStratum.MEDIUM), (CohortProvider.CLAUDE, LengthStratum.SHORT),
                  (CohortProvider.CODEX, LengthStratum.MEDIUM), (CohortProvider.CODEX, LengthStratum.SHORT)]:
        raise ValueError("all four provider/length strata are required")
    result = []
    for provider, stratum in strata:
        bucket = [c for c in candidates if (c.provider, c.stratum) == (provider, stratum)]
        if len(bucket) < per_stratum:
            raise ValueError("insufficient candidates in a required stratum")
        bucket.sort(key=lambda c: hashlib.sha256(seed.encode() + c.group).digest())
        result.extend(bucket[:per_stratum])
    return tuple(result)


def exclusions(paths: tuple[Path, ...], groups: dict, generation: str) -> tuple[set[bytes], list[dict]]:
    """Exclude every previously reviewed canonical group, including aliases."""
    result, manifest = set(), []
    for path in paths:
        raw = path.read_bytes()
        if len(raw) > 16 * 1024 * 1024:
            raise ValueError("excluded annotation file exceeds cap")
        specification = tomllib.loads(raw.decode())
        if specification.get("schema_version") != 1 or specification.get("generation") != generation:
            raise ValueError("excluded annotation generation/schema changed")
        if not isinstance(specification.get("cases"), list):
            raise ValueError("excluded annotation cases required")
        for case in specification["cases"]:
            identity = case.get("thread") if isinstance(case, dict) else None
            group = groups.get(identity)
            if (not isinstance(group, list) or len(group) != 32
                    or any(type(value) is not int or not 0 <= value <= 255 for value in group)):
                raise ValueError("excluded thread lacks a valid canonical group")
            result.add(bytes(group))
        manifest.append({"sha256": hashlib.sha256(raw).hexdigest(), "cases": len(specification["cases"])})
    return result, manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "exclude-annotations", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--also-exclude-annotations", type=Path, action="append", default=[])
    parser.add_argument("--seed", default="mash-topic-review-v2-20261001")
    parser.add_argument("--per-stratum", type=int, default=2)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError("use a fresh output path")
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private source packets must remain outside Git")
    dataset = load(args.database, args.partition_report)
    groups = json.loads(args.partition_report.read_text())["partition"]["source_ids"]
    excluded_groups, excluded_manifest = exclusions((args.exclude_annotations, *args.also_exclude_annotations), groups, dataset.generation)
    policy = {"seed": args.seed, "per_stratum": args.per_stratum,
              "minimum_users": 3, "maximum_users": 40, "short_maximum_users": 10,
              "maximum_turn_characters": 4096, "maximum_thread_utf8_bytes": 40000,
              "scope": "complete_nonempty_searchable_user_sequence", "reserved_groups": "excluded",
              "display_redaction": "standalone_credential_like_values_not_general_secret_scanning"}
    candidates = []
    packets = {}
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("BEGIN")
        for thread in dataset.threads:
            if thread.group in excluded_groups or thread.group[0] % 5 == 0:
                continue
            count = connection.execute(
                "SELECT COUNT(*) FROM messages WHERE conversation_id=? AND role='user' "
                "AND length(trim(COALESCE(search_projection,content)))>0", (thread.id,)).fetchone()[0]
            if not 3 <= count <= 40:
                continue
            turns = read_turns(connection, thread.id, dataset.generation)
            if not 3 <= len(turns) <= 40:
                continue
            if max(len(t.text) for t in turns) > 4096 or sum(len(t.text.encode()) for t in turns) > 40000:
                continue
            stratum = LengthStratum.SHORT if len(turns) <= 10 else LengthStratum.MEDIUM
            candidates.append(Candidate(thread.id, CohortProvider(thread.provider), thread.group, stratum))
            packets[thread.id] = {"thread": thread.id, "provider": thread.provider, "stratum": stratum,
                                  "source_sha256": source_hash(turns), "source_group": thread.group.hex(),
                                  "turns": [{"id": t.id, "ordinal": t.ordinal, "text": review_text(t.text)[0],
                                             "display_redacted": review_text(t.text)[1],
                                             "text_sha256": hashlib.sha256(t.text.encode()).hexdigest(),
                                             "stored_vector_available": t.vector is not None} for t in turns]}
    selected = select(candidates, policy["seed"], policy["per_stratum"])
    counts = {f"{provider}/{stratum}": sum((c.provider, c.stratum) == (provider, stratum) for c in candidates)
              for provider, stratum in sorted({(c.provider, c.stratum) for c in candidates})}
    report = {"schema_version": 1, "generation": dataset.generation, "input_fingerprint": dataset.fingerprint,
              "policy": policy, "eligible_strata": counts,
              "excluded_annotations_sha256": hashlib.sha256(args.exclude_annotations.read_bytes()).hexdigest(),
              "excluded_manifests": excluded_manifest,
              "partition_sha256": dataset.partition_sha256,
              "source_sha256": {name: hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
                                for name in ("review_packets.py", "data.py", "episodes.py", "uv.lock")},
              "cases": [packets[c.thread] for c in selected]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
        output.write("\n")


if __name__ == "__main__":
    main()
