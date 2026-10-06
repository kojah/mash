"""Small source-finding retrieval regression pilot, separate from turn clustering."""

import os
import tomllib
from pathlib import Path

import click
import numpy as np
import orjson
from pydantic import Field, model_validator

from build_token_weighting import digest
from corpus import Record
from followup_inputs import Text, windows
from local_encoder import Encoder
from token_weighting import Frequencies, Policy, encode_policies
from turn_artifact import generation


class Document(Record):
    id: str
    raw: str
    cleaned: str


class Query(Record):
    text: str
    relevant: tuple[str, ...] = Field(min_length=1)


class Corpus(Record):
    version: int
    documents: tuple[Document, ...]
    queries: tuple[Query, ...]

    @model_validator(mode="after")
    def valid(self) -> "Corpus":
        ids = {d.id for d in self.documents}
        if self.version != 1 or len(ids) != len(self.documents):
            raise ValueError("unsupported corpus version or duplicate document IDs")
        if not ids or any(not set(q.relevant) <= ids for q in self.queries):
            raise ValueError("query has an unknown source target")
        return self


class Ranking(Record):
    query_index: int
    target_rank: int
    relevant_at_five: float
    top_five: tuple[str, ...]


class Result(Record):
    corpus: str
    corpus_sha256: str
    encoder_generation: str
    documents: int
    queries: int
    truncated_inputs: int
    weighted_tokens: int
    changed_document_vectors: int
    uniform: tuple[Ranking, ...]
    function: tuple[Ranking, ...]


def rank(
    scores: np.ndarray, ids: tuple[str, ...], relevant: tuple[str, ...], index: int
) -> Ranking:
    # Stable source-order ties, fixed before looking at results.
    ordered = tuple(ids[i] for i in np.argsort(-scores, kind="stable"))
    targets = set(relevant)
    return Ranking(
        query_index=index,
        target_rank=min(
            i + 1 for i, identity in enumerate(ordered) if identity in targets
        ),
        relevant_at_five=len(set(ordered[:5]) & targets) / len(targets),
        top_five=ordered[:5],
    )


def evaluate(corpus_path: Path, encoder: Encoder) -> Result:
    raw = corpus_path.read_bytes()
    corpus = Corpus.model_validate(tomllib.loads(raw.decode()))
    ids = tuple(d.id for d in corpus.documents)
    vectors: dict[Policy, list[np.ndarray]] = {
        p: [] for p in (Policy.UNIFORM, Policy.FUNCTION)
    }
    truncated = weighted = 0
    for text in tuple(d.raw for d in corpus.documents) + tuple(
        q.text for q in corpus.queries
    ):
        bounded = windows(encoder.tokenizer, Text(text=text), 510, first_only=True)
        if not bounded:
            raise ValueError("retrieval input is empty")
        truncated += len(encoder.tokenizer.encode(text).ids) > 512
        points, coverage = encode_policies(
            encoder, bounded[0].text, Frequencies.from_messages(())
        )
        weighted += coverage[Policy.FUNCTION]
        for policy in vectors:
            vectors[policy].append(points[policy])
    ranked = {}
    for policy, points in vectors.items():
        matrix = np.stack(points)
        documents = matrix[: len(ids)]
        queries = matrix[len(ids) :]
        ranked[policy] = tuple(
            rank(documents @ query, ids, q.relevant, i)
            for i, (query, q) in enumerate(zip(queries, corpus.queries, strict=True))
        )
    # Recount document-only movement; query movement is deliberately independent.
    document_changed = sum(
        not np.array_equal(a, b)
        for a, b in zip(
            vectors[Policy.UNIFORM][: len(ids)],
            vectors[Policy.FUNCTION][: len(ids)],
            strict=True,
        )
    )
    return Result(
        corpus=corpus_path.stem,
        corpus_sha256=digest(raw),
        encoder_generation=generation(encoder),
        documents=len(ids),
        queries=len(corpus.queries),
        truncated_inputs=truncated,
        weighted_tokens=weighted,
        changed_document_vectors=document_changed,
        uniform=ranked[Policy.UNIFORM],
        function=ranked[Policy.FUNCTION],
    )


@click.command()
@click.option(
    "--corpus",
    type=click.Path(path_type=Path, exists=True),
    required=True,
    multiple=True,
)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(corpus: tuple[Path, ...], model: Path, output: Path) -> None:
    os.umask(0o077)
    encoder = Encoder(model)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    for path in corpus:
        result = evaluate(path, encoder)
        (output / f"{path.stem}.json").write_bytes(
            orjson.dumps(result.model_dump(mode="json"))
        )
        click.echo(
            f"{path.stem}: queries={result.queries} "
            f"weighted_tokens={result.weighted_tokens}"
        )


if __name__ == "__main__":
    main()
