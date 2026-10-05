"""Development-only bounded screen, isolated fit processes and typed reports."""

import hashlib
import multiprocessing as mp
import os
import resource
import time
from enum import StrEnum
from multiprocessing.connection import Connection
from pathlib import Path

import click
import numpy as np
import orjson
from sklearn.exceptions import ConvergenceWarning
from threadpoolctl import threadpool_limits

from alternatives import SETTINGS, Setting, partition
from clustering import Configuration, Method, cluster, result_from_partition
from corpus import Record, Split
from preparation import Prepared, prepare
from references import ThreadReference, load_references
from run_evaluation import ThreadResult, baseline, expand, thread_score

DEADLINE_SECONDS = 30


class Status(StrEnum):
    COMPLETE = "complete"
    TIMEOUT = "timeout"
    NONCONVERGED = "nonconverged"
    ERROR = "error"


class Outcome(Record):
    index: int
    status: Status
    score: ThreadResult | None = None
    peak_rss_mib: float | None = None


class Candidate(Record):
    setting: Setting | None
    outcomes: tuple[Outcome, ...]


class Screen(Record):
    corpus_sha256: str
    references_sha256: str
    protocol_sha256: str
    split: Split = Split.DEVELOPMENT
    deadline_seconds: int = DEADLINE_SECONDS
    candidates: tuple[Candidate, ...]
    windows: tuple[ThreadResult, ...]


def worker(
    channel: Connection,
    index: int,
    prepared: Prepared,
    reference: ThreadReference,
    thread_hash: str,
    provider: str,
    length: str,
    setting: Setting | None,
) -> None:
    """Return metrics only, never source text; process death remains observable."""
    with channel, threadpool_limits(limits=1):
        start = time.perf_counter()
        try:
            if setting is None:
                result = cluster(
                    prepared.vectors,
                    Configuration(Method.AVERAGE_LINK, 0.35),
                    prepared.substantive,
                )
            else:
                result = result_from_partition(
                    prepared.vectors,
                    partition(prepared.vectors, setting),
                    0.35,
                    prepared.substantive,
                )
            seconds = time.perf_counter() - start
            outcome = Outcome(
                index=index,
                status=Status.COMPLETE,
                score=thread_score(
                    expand(result, prepared),
                    reference,
                    prepared,
                    thread_hash,
                    provider,
                    length,
                    seconds,
                ),
            )
        except ConvergenceWarning:
            outcome = Outcome(index=index, status=Status.NONCONVERGED)
        except (ValueError, MemoryError, np.linalg.LinAlgError):
            outcome = Outcome(index=index, status=Status.ERROR)
        outcome = outcome.model_copy(
            update={
                "peak_rss_mib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
                / 1024
            }
        )
        channel.send_bytes(orjson.dumps(outcome.model_dump(mode="json")))


def isolated_fit(
    index: int,
    prepared: Prepared,
    reference: ThreadReference,
    thread_hash: str,
    provider: str,
    length: str,
    setting: Setting | None,
    deadline: float = DEADLINE_SECONDS,
) -> Outcome:
    """Linux research adapter; timeout terminates/reaps only its owned worker."""
    context = mp.get_context("fork")
    receiver, sender = context.Pipe(duplex=False)
    process = context.Process(
        target=worker,
        args=(
            sender,
            index,
            prepared,
            reference,
            thread_hash,
            provider,
            length,
            setting,
        ),
    )
    process.start()
    sender.close()
    try:
        if receiver.poll(deadline):
            try:
                return Outcome.model_validate_json(receiver.recv_bytes())
            except EOFError:
                return Outcome(index=index, status=Status.ERROR)
        # Stop immediately at the deadline; do not let a late worker score or
        # write into a closed pipe while the parent waits for cleanup.
        process.terminate()
        return Outcome(index=index, status=Status.TIMEOUT)
    finally:
        receiver.close()
        process.join(timeout=1)
        if process.is_alive():
            process.terminate()
            process.join(timeout=1)
        if process.is_alive():
            process.kill()
            process.join()
        process.close()


@click.command()
@click.option("--corpus", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--references", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(corpus: Path, references: Path, output: Path) -> None:
    """The second study never exposes a held-out evaluation option."""
    if not output.resolve().is_relative_to(Path("/home/james/scratch/mash-artifacts")):
        raise click.ClickException("output must be in the private artifact directory")
    try:
        source, judgments, reference_hash = load_references(references, corpus)
    except (OSError, ValueError) as error:
        raise click.ClickException(
            f"cannot validate frozen inputs ({type(error).__name__})"
        ) from error
    protocol = (
        Path(__file__).resolve().parents[2] / "docs/message-clustering-alternatives.md"
    )
    protocol_hash = hashlib.sha256(protocol.read_bytes()).hexdigest()
    results: dict[Setting | None, list[Outcome]] = {
        setting: [] for setting in (*SETTINGS, None)
    }
    windows: list[ThreadResult] = []
    by_index = {reference.index: reference for reference in judgments.threads}
    for index, entry in enumerate(source.selected):
        if entry.split != Split.DEVELOPMENT:
            continue
        reference = by_index[index]
        prepared = prepare(entry.thread, reference, context=True)
        thread_hash = hashlib.sha256(entry.thread.id.encode()).hexdigest()
        for setting in results:
            outcome = isolated_fit(
                index,
                prepared,
                reference,
                thread_hash,
                entry.thread.provider.value,
                entry.thread.length.value,
                setting,
            )
            results[setting].append(outcome)
            name = str(setting) if setting else "average_link_reference"
            click.echo(f"Thread {index}, {name}: {outcome.status}")
        raw = prepare(entry.thread, reference, context=False)
        windows.append(
            thread_score(
                expand(baseline(raw, 8), raw),
                reference,
                raw,
                thread_hash,
                entry.thread.provider.value,
                entry.thread.length.value,
                0,
            )
        )
    report = Screen(
        corpus_sha256=judgments.corpus_sha256,
        references_sha256=reference_hash,
        protocol_sha256=protocol_hash,
        candidates=tuple(
            Candidate(setting=setting, outcomes=tuple(outcomes))
            for setting, outcomes in results.items()
        ),
        windows=tuple(windows),
    )
    encoded = orjson.dumps(report.model_dump(mode="json"))
    with os.fdopen(
        os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb"
    ) as file:
        file.write(encoded)
    click.echo(f"Report SHA-256 {hashlib.sha256(encoded).hexdigest()}")
    for candidate in report.candidates:
        scores = [outcome.score for outcome in candidate.outcomes]
        if all(score is not None for score in scores):
            f1 = float(np.mean([score.overlapping.f1 for score in scores if score]))
            click.echo(f"{candidate.setting}: macro F1 {f1:.3f}")
        else:
            click.echo(f"{candidate.setting}: incomplete; not ranked")


if __name__ == "__main__":
    main()
