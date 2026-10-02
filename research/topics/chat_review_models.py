"""Typed development review models, independent of inference or scientific libs."""
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum


class Importance(StrEnum):
    PRIMARY = 'primary'
    SECONDARY = 'secondary'


class TaskKind(StrEnum):
    THREAD = 'specific-thread-recovery'
    DISCOVERY = 'broad-subject-discovery'
    MULTIPLE = 'multiple-subject-exploration'
    ENTITY = 'rare-technical-entity'
    SCOPED = 'scoped-lookup'


class MetadataField(StrEnum):
    PROVIDER = 'provider'
    TIME = 'time_range'
    NODE = 'node'


@dataclass(frozen=True)
class Turn:
    id: str
    ordinal: int
    role: str
    text: str
    redacted: bool


@dataclass(frozen=True)
class Document:
    id: str
    turns: tuple[Turn, ...]


@dataclass(frozen=True)
class Packet:
    sha256: str
    documents: tuple[Document, ...]


@dataclass(frozen=True)
class Evidence:
    turn_id: str
    quote: str


@dataclass(frozen=True)
class Subject:
    name: str
    importance: Importance
    broader_subjects: tuple[str, ...]
    evidence: tuple[Evidence, ...]
    uncertainty: str


@dataclass(frozen=True)
class TimeRange:
    start: datetime | None
    end: datetime | None


@dataclass(frozen=True)
class Constraints:
    provider: str | None
    time_range: TimeRange | None
    node: str | None
    unavailable: frozenset[MetadataField]


@dataclass(frozen=True)
class NavigationTask:
    request: str
    kind: TaskKind
    evidence: tuple[Evidence, ...]
    constraints: Constraints


@dataclass(frozen=True)
class Judgment:
    id: str
    sufficient_context: bool
    subjects: tuple[Subject, ...]
    tasks: tuple[NavigationTask, ...]
    notes: str


@dataclass(frozen=True)
class Reviewer:
    identity: str
    human_attestation: bool
    prior_source_exposure: bool
    candidate_output_exposure: bool


@dataclass(frozen=True)
class Review:
    packet_sha256: str
    reviewer: Reviewer
    documents: tuple[Judgment, ...]
