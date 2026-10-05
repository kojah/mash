"""Isolated grouping/update cost; scaling fixtures are not quality evidence."""
# ruff: noqa: E402
# The clock must start before numerical/research imports to report cold import cost.

import hashlib
import os
import resource
import sys
import time
from dataclasses import asdict
from enum import StrEnum
from pathlib import Path

IMPORT_START = time.perf_counter()

import click
import numpy as np
import orjson
from sklearn.cluster import Birch
from threadpoolctl import threadpool_limits

from clustering import (
    Configuration,
    Method,
    OnlineLeader,
    VectorMatrix,
    cluster,
    normalized_vectors,
)
from corpus import Record
from frozen_settings import FrozenSettings

IMPORT_SECONDS = time.perf_counter() - IMPORT_START


class Operation(StrEnum):
    WARM_REBUILD = "warm_rebuild"
    APPEND_REBUILD = "append_rebuild"
    EDIT_REBUILD = "edit_rebuild"
    DELETE_REBUILD = "delete_rebuild"


class Timing(Record):
    operation: Operation
    milliseconds: tuple[float, ...]
    p95_milliseconds: float
    median_milliseconds: float
    result_sha256: str
    deterministic: bool


class Benchmark(Record):
    method: Method
    messages: int
    synthetic_scaling: bool
    controlled_updates: bool = True
    import_seconds: float
    peak_rss_mib: float
    timings: tuple[Timing, ...]


def native_insert_time(
    vectors: VectorMatrix, configuration: Configuration
) -> float | None:
    """One observed native insertion; excludes reassignment/publication costs.

    The existing tree/centroids are fit before timing. This is not an end-to-end
    update or a p95 estimate, and does not imply native edit/deletion support.
    """
    if configuration.method == Method.LEADER:
        leader = OnlineLeader(configuration.cosine_threshold)
        for vector in vectors:
            leader.append(vector)
        start = time.perf_counter()
        leader.append(vectors[-1])
    elif configuration.method == Method.BIRCH:
        birch = Birch(
            threshold=configuration.radius,
            branching_factor=50,
            n_clusters=None,
            compute_labels=False,
        )
        for vector in vectors:
            birch.partial_fit(vector.reshape(1, -1))
        start = time.perf_counter()
        birch.partial_fit(vectors[-1].reshape(1, -1))
    else:
        return None
    return (time.perf_counter() - start) * 1000


@click.command()
@click.option("--fixture", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--settings", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--method", type=click.Choice([method.value for method in Method]), required=True
)
@click.option("--messages", type=click.Choice(["1000", "5000", "real"]), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(
    fixture: Path, settings: Path, method: str, messages: str, output: Path
) -> None:
    """Run five warm repeats plus controlled append/edit/delete rebuilds."""
    if not output.resolve().is_relative_to(
        Path("/home/james/scratch/mash-artifacts").resolve()
    ):
        raise click.ClickException("output must be in the private artifact directory")
    choices = FrozenSettings.model_validate_json(settings.read_bytes())
    selection = next(
        choice for choice in choices.selections if choice.configuration.method == method
    )
    with np.load(fixture, allow_pickle=False) as stored:
        base = stored["vectors"].astype(np.float64)
        support = stored["substantive"].astype(np.bool_)
    count = len(base) if messages == "real" else int(messages)
    scaling = count > len(base)
    indices = np.arange(count) % len(base)
    vectors = base[indices].copy()
    eligible = support[indices].copy()
    if scaling:
        # Avoid all-zero distance plateaus while preserving realistic geometry.
        # These are controlled scaling points, never inferred real messages.
        vectors += np.random.default_rng(20261005).normal(0, 1e-6, vectors.shape)
        vectors = normalized_vectors(vectors)
    variants = (
        (Operation.WARM_REBUILD, vectors, eligible, 10),
        (
            Operation.APPEND_REBUILD,
            np.vstack((vectors, base[-1])),
            np.append(eligible, True),
            3,
        ),
        (
            Operation.EDIT_REBUILD,
            np.vstack((vectors[: count // 2], base[-1], vectors[count // 2 + 1 :])),
            eligible,
            3,
        ),
        (
            Operation.DELETE_REBUILD,
            np.delete(vectors, count // 2, axis=0),
            np.delete(eligible, count // 2),
            3,
        ),
    )
    timings: list[Timing] = []
    with threadpool_limits(limits=1):
        # Imports/input decode are reported separately from warm grouping.
        cluster(vectors, selection.configuration, eligible)
        for operation, data, mask, repetitions in variants:
            values: list[float] = []
            hashes: set[str] = set()
            for _ in range(repetitions):
                start = time.perf_counter()
                result = cluster(data, selection.configuration, mask)
                values.append((time.perf_counter() - start) * 1000)
                hashes.add(hashlib.sha256(orjson.dumps(asdict(result))).hexdigest())
            timings.append(
                Timing(
                    operation=operation,
                    milliseconds=tuple(values),
                    p95_milliseconds=float(np.percentile(values, 95)),
                    median_milliseconds=float(np.median(values)),
                    result_sha256=min(hashes),
                    deterministic=len(hashes) == 1,
                )
            )
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    report = Benchmark(
        method=Method(method),
        messages=count,
        synthetic_scaling=scaling,
        import_seconds=IMPORT_SECONDS,
        peak_rss_mib=rss / (1024 * 1024 if sys.platform == "darwin" else 1024),
        timings=tuple(timings),
    )
    encoded = orjson.dumps(report.model_dump(mode="json"))
    with os.fdopen(
        os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb"
    ) as file:
        file.write(encoded)
    click.echo(
        f"{method}: n={count}, p95={timings[0].p95_milliseconds:.1f}ms, "
        f"RSS={report.peak_rss_mib:.1f}MiB; synthetic={scaling}"
    )


if __name__ == "__main__":
    main()
