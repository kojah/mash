"""Stable reads, frontmatter and conservative identity/deletion reconciliation."""

import hashlib
import math
import os
import re
import stat
import time
import uuid
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path, PurePosixPath
from urllib.parse import quote

import orjson
import yaml
from mash_sdk import Object, Role
from pydantic import JsonValue, TypeAdapter

from .config import Settings
from .registry import Identity, Registry

METADATA = TypeAdapter(dict[str, JsonValue])
EMBED = re.compile(r"!\[\[([^\]\n]+)\]\]|!\[[^\]\n]*\]\(([^)\n]+)\)")
TAG = re.compile(r"(?<![\w/#])#([\w/-]+)")


@dataclass(frozen=True)
class File:
    path: str
    device: int
    inode: int
    digest: str
    modified_ms: int
    text: str
    title: str
    created_at: int | None
    properties: dict[str, JsonValue]
    tags: list[str]


@dataclass(frozen=True)
class Scan:
    documents: list[Object]
    deleted: list[str]
    scanned: int
    excluded: int
    issues: list[str]


def properties(text: str) -> tuple[dict[str, JsonValue], str]:
    lines = text.lstrip("\ufeff").splitlines(keepends=True)
    if not lines or lines[0].strip() != "---":
        return {}, text
    end = next((i for i in range(1, len(lines)) if lines[i].strip() == "---"), None)
    if end is None:
        raise ValueError("frontmatter is incomplete")
    value = yaml.safe_load("".join(lines[1:end]))
    if value is None:
        return {}, "".join(lines[end + 1 :])
    if not isinstance(value, dict):
        raise ValueError("frontmatter must be a mapping")
    # Bound YAML aliases and reject nonfinite values before orjson can turn
    # NaN into null. The original Markdown is always retained unchanged.
    queue = [value]
    count = 0
    while queue:
        item = queue.pop()
        count += 1
        if count + len(queue) > 4096:
            raise ValueError("frontmatter is too complex")
        if isinstance(item, dict):
            if not all(isinstance(key, str) for key in item):
                raise ValueError("frontmatter keys must be strings")
            queue.extend(item.values())
        elif isinstance(item, list):
            queue.extend(item)
        elif isinstance(item, float) and not math.isfinite(item):
            raise ValueError("nonfinite frontmatter value")
    normalized = METADATA.validate_json(orjson.dumps(value))
    return normalized, "".join(lines[end + 1 :])


def creation(properties: dict[str, JsonValue]) -> int | None:
    value = properties.get("created")
    if not isinstance(value, str):
        return None
    try:
        moment = datetime.fromisoformat(value.replace("Z", "+00:00"))
        return int(moment.timestamp() * 1000) if moment.tzinfo is not None else None
    except (ValueError, OverflowError):
        return None


def tags(properties: dict[str, JsonValue], body: str) -> list[str]:
    value = properties.get("tags", [])
    supplied = value.split(",") if isinstance(value, str) else value
    found = (
        {
            item.strip().removeprefix("#")
            for item in supplied
            if isinstance(item, str) and item.strip()
        }
        if isinstance(supplied, list)
        else set()
    )
    fence = False
    for line in body.splitlines():
        if line.lstrip().startswith(("```", "~~~")):
            fence = not fence
        elif not fence:
            found.update(TAG.findall(re.sub(r"`[^`]*`", "", line)))
    return sorted(found)


def read(settings: Settings, path: str) -> File:
    # Walk directory descriptors without following symlinks, including during
    # a concurrent path replacement. Only the selected vault can supply bytes.
    directory = os.open(settings.vault, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    descriptor = None
    try:
        parts = PurePosixPath(path).parts
        for part in parts[:-1]:
            next_dir = os.open(
                part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory
            )
            os.close(directory)
            directory = next_dir
        descriptor = os.open(
            parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory
        )
        with os.fdopen(descriptor, "rb") as stream:
            descriptor = None
            before = os.fstat(stream.fileno())
            if not stat.S_ISREG(before.st_mode):
                raise ValueError("note is not a regular file")
            data = stream.read(settings.max_note_bytes + 1)
            after = os.fstat(stream.fileno())
        if len(data) > settings.max_note_bytes:
            raise ValueError("note exceeds configured byte limit")
        if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError("note changed during read")
        text = data.decode("utf-8")
        frontmatter, body = properties(text)
        heading = next(
            (line[2:].strip() for line in body.splitlines() if line.startswith("# ")),
            None,
        )
        title = frontmatter.get("title")
        return File(
            path,
            after.st_dev,
            after.st_ino,
            hashlib.sha256(data).hexdigest(),
            after.st_mtime_ns // 1_000_000,
            text,
            title if isinstance(title, str) and title else heading or Path(path).stem,
            creation(frontmatter),
            frontmatter,
            tags(frontmatter, body),
        )
    finally:
        if descriptor is not None:
            os.close(descriptor)
        os.close(directory)


def identities(
    files: list[File], known: list[Identity], paths: set[str]
) -> list[Identity]:
    """Inode first, then surviving path (atomic saves), then unique moved content.

    A copy is not a rename while the old path exists. Ambiguous content matches
    get fresh identities; we never merge distinct notes or rewrite frontmatter.
    """
    assigned: dict[str, Identity] = {}
    used: set[str] = set()
    by_path = {file.path: file for file in files}
    by_inode: dict[tuple[int, int], list[Identity]] = defaultdict(list)
    by_name: dict[str, list[Identity]] = defaultdict(list)
    by_digest: dict[str, list[Identity]] = defaultdict(list)
    for note in known:
        by_inode[note.device, note.inode].append(note)
        by_name[note.path].append(note)
        by_digest[note.digest].append(note)
    counts = Counter((file.device, file.inode) for file in files)
    for file in files:
        candidates = [
            n
            for n in by_inode[file.device, file.inode]
            if (
                n.path == file.path
                or n.path not in paths
                or n.path in by_path
                and (by_path[n.path].device, by_path[n.path].inode)
                != (n.device, n.inode)
            )
        ]
        if file.inode and counts[file.device, file.inode] == 1 and len(candidates) == 1:
            assigned[file.path] = candidates[0]
            used.add(candidates[0].id)
    for file in files:
        if file.path in assigned:
            continue
        candidates = [n for n in by_name[file.path] if n.id not in used]
        if len(candidates) == 1:
            assigned[file.path] = candidates[0]
            used.add(candidates[0].id)
    new_counts = Counter(f.digest for f in files if f.path not in assigned)
    for file in files:
        if file.path in assigned:
            continue
        candidates = [
            n
            for n in by_digest[file.digest]
            if n.path not in paths and n.id not in used
        ]
        if new_counts[file.digest] == 1 and len(candidates) == 1:
            assigned[file.path] = candidates[0]
            used.add(candidates[0].id)
    return [
        Identity(
            assigned[file.path].id if file.path in assigned else uuid.uuid4().hex,
            file.path,
            file.device,
            file.inode,
            file.digest,
            file.created_at,
            None,
        )
        for file in files
    ]


def embeds(settings: Settings, file: File) -> list[JsonValue]:
    refs: list[JsonValue] = []
    for match in EMBED.finditer(file.text):
        target = (match.group(1) or match.group(2)).split("|", 1)[0].split("#", 1)[0]
        resolved: str | None = None
        if target and ":" not in target:
            for base in [(settings.vault / file.path).parent, settings.vault]:
                candidate = (base / target).resolve()
                if candidate.is_relative_to(settings.vault) and candidate.is_file():
                    resolved = candidate.relative_to(settings.vault).as_posix()
                    break
        refs.append({"target": target, "path": resolved})
    return refs


def scan(settings: Settings, registry: Registry, *, now: float | None = None) -> Scan:
    now = time.time() if now is None else now
    issues: list[str] = []
    paths: set[str] = set()

    def failed(error: OSError) -> None:
        issues.append(f"vault enumeration failed ({type(error).__name__})")

    for folder in settings.folders:
        root = settings.vault / folder
        if (
            not root.is_dir()
            or root.is_symlink()
            or not root.resolve().is_relative_to(settings.vault)
        ):
            issues.append(f"folder unavailable: {folder}")
            continue
        for parent, directories, names in os.walk(
            root, followlinks=False, onerror=failed
        ):
            for name in list(directories):
                path = Path(parent) / name
                if name in {".obsidian", ".trash", ".git"}:
                    directories.remove(name)
                elif path.is_symlink():
                    directories.remove(name)
                    issues.append("symlink directory excluded from enumeration")
            if not settings.recursive:
                directories.clear()
            for name in names:
                if Path(name).suffix.lower() == ".md":
                    relative = (
                        (Path(parent) / name).relative_to(settings.vault).as_posix()
                    )
                    if settings.selected(relative):
                        paths.add(relative)
    files: list[File] = []
    for path in sorted(paths):
        try:
            files.append(read(settings, path))
        except (
            OSError,
            UnicodeError,
            ValueError,
            yaml.YAMLError,
            orjson.JSONEncodeError,
        ) as error:
            issues.append(f"note unavailable: {path} ({type(error).__name__})")
    known = registry.active()
    existing = paths | {
        n.path for n in known if os.path.lexists(settings.vault / n.path)
    }
    observed = identities(files, known, existing)
    seen = {n.id for n in observed}
    missing = [n for n in known if n.id not in seen and settings.selected(n.path)]
    # Failed enumeration/read cannot establish any deletion. A successful scan
    # starts the grace period; another successful scan must confirm absence.
    registry.observe(observed, missing if not issues else [], now)
    cutoff = (
        None
        if settings.history_days is None
        else int((now - settings.history_days * 86400) * 1000)
    )

    def included(created: int | None) -> bool:
        return cutoff is None or created is not None and created >= cutoff

    deleted = [
        n.id
        for n in missing
        if not issues
        and n.missing_since is not None
        and now - n.missing_since >= settings.deletion_grace_seconds
        and included(n.created_at)
    ]
    documents = [
        Object(
            id=identity.id,
            role=Role.document,
            kind="obsidian:note",
            title=file.title,
            content=file.text,
            created_at=file.created_at,
            updated_at=file.modified_ms,
            metadata={
                "vault_id": settings.vault_id,
                "path": file.path,
                "frontmatter": file.properties,
                "tags": file.tags,
                "embeds": embeds(settings, file),
                "obsidian_uri": (
                    f"obsidian://open?vault={quote(settings.vault.name, safe='')}"
                    f"&file={quote(file.path, safe='')}"
                ),
            },
        )
        for file, identity in zip(files, observed, strict=True)
        if included(file.created_at)
    ]
    return Scan(documents, deleted, len(paths), len(files) - len(documents), issues)
