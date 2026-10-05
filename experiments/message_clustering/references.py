"""Validate blind diagnostic labels before any candidate output is visible."""

import hashlib
import tomllib
from enum import StrEnum
from pathlib import Path

from pydantic import Field

from corpus import Corpus, Record
from evaluation import Pair, Verdict

IndexPair = tuple[int, int]


class Case(StrEnum):
    RETURNING = "returning"
    SHORT_REPLY = "short_reply"
    MIXED = "mixed"


class Kind(StrEnum):
    SUBSTANTIVE = "substantive"
    REFERENTIAL = "referential"
    BOILERPLATE = "boilerplate"
    UNCERTAIN = "uncertain"


class ThreadReference(Record):
    index: int = Field(ge=0)
    related: tuple[IndexPair, ...]
    unrelated: tuple[IndexPair, ...]
    ambiguous: tuple[IndexPair, ...] = ()
    referential: tuple[int, ...] = ()
    boilerplate: tuple[int, ...] = ()
    uncertain: tuple[int, ...] = ()
    returning: tuple[IndexPair, ...] = ()
    short_reply: tuple[IndexPair, ...] = ()
    mixed: tuple[IndexPair, ...] = ()
    notes: str

    def pairs(self, case: Case | None = None) -> tuple[Pair, ...]:
        selected = (
            None
            if case is None
            else {tuple(sorted(pair)) for pair in getattr(self, case.value)}
        )
        return tuple(
            Pair(left, right, verdict)
            for verdict, identities in (
                (Verdict.RELATED, self.related),
                (Verdict.UNRELATED, self.unrelated),
                (Verdict.AMBIGUOUS, self.ambiguous),
            )
            for left, right in identities
            if selected is None or tuple(sorted((left, right))) in selected
        )

    def kind(self, index: int) -> Kind:
        if index in self.referential:
            return Kind.REFERENTIAL
        if index in self.boilerplate:
            return Kind.BOILERPLATE
        return Kind.UNCERTAIN if index in self.uncertain else Kind.SUBSTANTIVE

    @property
    def reviewed_indices(self) -> frozenset[int]:
        return frozenset(
            index for pair in self.pairs() for index in (pair.left, pair.right)
        )


class References(Record):
    schema_version: int
    corpus_sha256: str
    provenance: str
    threads: tuple[ThreadReference, ...]

    def validate_corpus(self, corpus: Corpus) -> None:
        if self.schema_version != 1:
            raise ValueError("unknown reference schema")
        if {thread.index for thread in self.threads} != set(
            range(len(corpus.selected))
        ):
            raise ValueError("every selected thread needs one reference record")
        if len(self.threads) != len(corpus.selected):
            raise ValueError("duplicate thread reference")
        for reference in self.threads:
            messages = corpus.selected[reference.index].thread.messages
            seen: set[IndexPair] = set()
            seen_text: set[tuple[str, str]] = set()
            for pair in reference.pairs():
                identity = (min(pair.left, pair.right), max(pair.left, pair.right))
                if identity[0] < 0 or identity[1] >= len(messages):
                    raise ValueError("reference index outside frozen thread")
                if identity[0] == identity[1] or identity in seen:
                    raise ValueError("duplicate/self reference pair")
                seen.add(identity)
                left = hashlib.sha256(
                    messages[pair.left].projection.encode()
                ).hexdigest()
                right = hashlib.sha256(
                    messages[pair.right].projection.encode()
                ).hexdigest()
                text_pair = (min(left, right), max(left, right))
                if left == right or text_pair in seen_text:
                    raise ValueError("duplicate projections would inflate judged pairs")
                seen_text.add(text_pair)
            for case in Case:
                if any(
                    tuple(sorted(pair)) not in seen
                    for pair in getattr(reference, case.value)
                ):
                    raise ValueError("case tags must refer to judged pairs")
            kinds = reference.referential + reference.boilerplate + reference.uncertain
            if len(set(kinds)) != len(kinds) or any(
                index < 0 or index >= len(messages) for index in kinds
            ):
                raise ValueError("invalid or overlapping message kinds")


def load_references(path: Path, corpus_path: Path) -> tuple[Corpus, References, str]:
    """Verify source bytes and labels; return a stable label-file digest."""
    corpus_bytes, reference_bytes = corpus_path.read_bytes(), path.read_bytes()
    corpus = Corpus.model_validate_json(corpus_bytes)
    references = References.model_validate(tomllib.loads(reference_bytes.decode()))
    if references.corpus_sha256 != hashlib.sha256(corpus_bytes).hexdigest():
        raise ValueError("references belong to a different corpus")
    references.validate_corpus(corpus)
    return corpus, references, hashlib.sha256(reference_bytes).hexdigest()
