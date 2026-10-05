"""The held-out command accepts only development-selected settings."""

import hashlib
import os
from pathlib import Path

import click
import numpy as np
import orjson
from pydantic import TypeAdapter

from clustering import CONFIGURATIONS, Configuration, Method
from corpus import Record, Split


class Selection(Record):
    configuration: Configuration
    context: bool
    development_f1: float


class FrozenSettings(Record):
    corpus_sha256: str
    references_sha256: str
    development_reports_sha256: tuple[str, ...]
    selections: tuple[Selection, ...]

    def verify(self, corpus_sha256: str, references_sha256: str) -> None:
        if (
            self.corpus_sha256 != corpus_sha256
            or self.references_sha256 != references_sha256
        ):
            raise ValueError("settings do not match the frozen corpus/references")
        if len(self.selections) != len(Method) or {
            selection.configuration.method for selection in self.selections
        } != set(Method):
            raise ValueError("one setting per method must be frozen")
        if any(
            selection.configuration not in CONFIGURATIONS
            for selection in self.selections
        ):
            raise ValueError("selected setting outside the preregistered grid")


@click.command()
@click.option(
    "--report",
    type=click.Path(path_type=Path, exists=True),
    multiple=True,
    required=True,
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(report: tuple[Path, ...], output: Path) -> None:
    """Maximize development macro F1 per method, without reading held-out output."""
    # Local import avoids a cycle with the runner that verifies these settings.
    from run_evaluation import Report

    if not output.resolve().is_relative_to(
        Path("/home/james/scratch/mash-artifacts").resolve()
    ):
        raise click.ClickException("output must be in the private artifact directory")
    payloads = tuple(path.read_bytes() for path in report)
    reports = tuple(Report.model_validate_json(payload) for payload in payloads)
    if (
        not reports
        or len(reports) > 2
        or any(r.split != Split.DEVELOPMENT for r in reports)
    ):
        raise click.ClickException(
            "only the bounded development reports may select settings"
        )
    if len({(r.corpus_sha256, r.references_sha256) for r in reports}) != 1:
        raise click.ClickException(
            "development reports have different corpus/references"
        )
    choices: dict[Method, Selection] = {}
    for results in reports:
        for candidate in results.candidates:
            if not candidate.configuration.startswith("{"):
                continue
            configuration = TypeAdapter(Configuration).validate_json(
                candidate.configuration
            )
            f1 = float(np.mean([thread.overlapping.f1 for thread in candidate.threads]))
            choice = Selection(
                configuration=configuration, context=results.context, development_f1=f1
            )
            previous = choices.get(configuration.method)
            if previous is None or choice.development_f1 > previous.development_f1:
                choices[configuration.method] = choice
    settings = FrozenSettings(
        corpus_sha256=reports[0].corpus_sha256,
        references_sha256=reports[0].references_sha256,
        development_reports_sha256=tuple(
            hashlib.sha256(payload).hexdigest() for payload in payloads
        ),
        selections=tuple(choices[method] for method in Method),
    )
    settings.verify(settings.corpus_sha256, settings.references_sha256)
    encoded = orjson.dumps(settings.model_dump(mode="json"))
    with os.fdopen(
        os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb"
    ) as file:
        file.write(encoded)
    click.echo(f"Frozen settings SHA-256 {hashlib.sha256(encoded).hexdigest()}")
    for selection in settings.selections:
        click.echo(
            f"{selection.configuration}: context={selection.context}; "
            f"dev F1={selection.development_f1:.3f}"
        )


if __name__ == "__main__":
    main()
