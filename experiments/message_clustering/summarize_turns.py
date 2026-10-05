"""Aggregate sparse turn scores, including negative-only threads and fixed baselines."""

from pathlib import Path

import click
import numpy as np
import orjson

from clustering import Configuration, Method, cluster
from corpus import Record, Split
from evaluation import evaluate_pairs
from preparation import prepare
from references import load_references
from run_evaluation import baseline, expand
from run_turns import Report, macro
from turn_evaluation import Counts, Fit, Inputs, cross_pairs
from turns import extract


class Aggregate(Record):
    name: str
    complete: int
    failures: int
    macro_all_f1: float
    macro_cross_positive_f1: float
    micro_all: Counts
    micro_cross: Counts


def aggregate(name: str, outcomes: tuple[Fit, ...]) -> Aggregate:
    all_counts = [fit.all_pairs for fit in outcomes if fit.all_pairs]
    cross_counts = [fit.cross_turn for fit in outcomes if fit.cross_turn]

    def total(counts: list[Counts]) -> Counts:
        return Counts(
            **{
                key: sum(getattr(value, key) for value in counts)
                for key in Counts.model_fields
            }
        )

    return Aggregate(
        name=name,
        complete=len(all_counts),
        failures=len(outcomes) - len(all_counts),
        macro_all_f1=float(np.mean([value.f1 for value in all_counts])),
        macro_cross_positive_f1=macro(outcomes),
        micro_all=total(all_counts),
        micro_cross=total(cross_counts),
    )


@click.command()
@click.option("--report", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--corpus", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--references", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(report: Path, corpus: Path, references: Path, output: Path) -> None:
    if not output.resolve().is_relative_to(Path("/home/james/scratch/mash-artifacts")):
        raise click.ClickException("use private artifact directory")
    results = Report.model_validate_json(report.read_bytes())
    source, labels, reference_hash = load_references(references, corpus)
    if (
        results.corpus_sha256 != labels.corpus_sha256
        or results.references_sha256 != reference_hash
    ):
        raise click.ClickException("inputs differ from recorded experiment")
    summaries = [
        aggregate(f"{candidate.recipe}/{candidate.setting}", candidate.outcomes)
        for candidate in results.candidates
    ]
    summaries.extend(
        aggregate(name, outcomes) for name, outcomes in results.controls.items()
    )
    summaries.append(aggregate("turn_bundling", results.bundling))
    for name in (
        "context_message_reference",
        "window_8_reference",
        "whole_thread_reference",
    ):
        outcomes = []
        for reference in labels.threads:
            selected = source.selected[reference.index]
            if selected.split != Split.DEVELOPMENT:
                continue
            thread = selected.thread
            prepared = prepare(
                thread, reference, context=name == "context_message_reference"
            )
            result = (
                cluster(
                    prepared.vectors,
                    Configuration(Method.AVERAGE_LINK, 0.35),
                    prepared.substantive,
                )
                if name == "context_message_reference"
                else baseline(
                    prepared,
                    8 if name == "window_8_reference" else len(prepared.vectors),
                )
            )
            expanded = expand(result, prepared)
            dummy = Inputs(thread, extract(thread), (), np.empty((0, 384)), reference)
            outcomes.append(
                Fit(
                    status="complete",
                    all_pairs=Counts.from_metrics(
                        evaluate_pairs(expanded, reference.pairs())
                    ),
                    cross_turn=Counts.from_metrics(
                        evaluate_pairs(expanded, cross_pairs(dummy))
                    ),
                )
            )
        summaries.append(aggregate(name, tuple(outcomes)))
    with output.open("xb") as file:
        file.write(orjson.dumps([summary.model_dump() for summary in summaries]))
    for summary in summaries:
        click.echo(
            f"{summary.name}: complete={summary.complete}, "
            f"all macro F1={summary.macro_all_f1:.3f}, "
            f"cross macro F1={summary.macro_cross_positive_f1:.3f}, "
            f"cross micro P/R/F1={summary.micro_cross.precision:.3f}/"
            f"{summary.micro_cross.recall:.3f}/{summary.micro_cross.f1:.3f}"
        )


if __name__ == "__main__":
    main()
