"""Inspect all six cells and run cold-cache real-model isolation probes."""

import hashlib
import os
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from ablation_evidence import fit
from build_local_representation import Artifact, build, name, source
from corpus import Record, Role
from experiment_study import Context, Contract, Setting
from local_encoder import Encoder
from local_representation import pooling
from run_turn_ablation import AblationReport, matrix
from turn_artifact import Store
from turn_study_corpus import Cohort, Phase


class Audit(Record):
    histories: int
    artifacts: int
    weighted_points: dict[str, int]
    changed_weighted_vectors: dict[str, int]
    unchanged_unweighted_points: dict[str, int]
    changed_context_points: int
    isolation_histories: tuple[int, ...]
    isolation_settings_checked: int
    limitation: str = (
        "Real-model isolation probes cover one short and one long history. "
        "All cells use own-history immutable pooled/user parent vectors; "
        "this does not test independent regeneration of those historical parents."
    )


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(parent: Path, artifacts: Path, model: Path, output: Path) -> None:
    os.umask(0o077)
    contract = Contract.model_validate_json((artifacts / "contract.json").read_bytes())
    cohort = Cohort.model_validate_json(
        (parent / "turn-study-fleet-verified-20261005/cohort.json").read_bytes()
    )
    entries = tuple(e for e in cohort.entries if e.phase == Phase.DEVELOPMENT)
    weighted = {c.value: 0 for c in Context}
    changed = weighted.copy()
    unchanged = weighted.copy()
    context_changed = 0
    for entry in entries:
        original_users = None
        for context in Context:
            built = []
            matrices = []
            for boilerplate in (False, True):
                path = artifacts / (
                    name(
                        entry.captured.index,
                        Setting(context=context, boilerplate=boilerplate),
                    )
                    + ".json"
                )
                item = Artifact.model_validate_json(path.read_bytes())
                raw = path.with_suffix(".npz").read_bytes()
                if (
                    hashlib.sha256(raw).hexdigest() != item.matrix_sha256
                    or item.protocol_sha256 != contract.protocol_sha256
                ):
                    raise ValueError("artifact hash/contract changed")
                built.append(item)
                matrices.append(matrix(raw)[0])
            if built[0].units != built[1].units:
                raise ValueError("weighting changed input text/attention")
            mask = np.array(
                [bool(pooling(u, True).downweighted) for u in built[0].units]
            )
            np.testing.assert_array_equal(matrices[0][~mask], matrices[1][~mask])
            weighted[context.value] += int(mask.sum())
            changed[context.value] += sum(
                not np.array_equal(a, b)
                for a, b in zip(matrices[0][mask], matrices[1][mask], strict=True)
            )
            unchanged[context.value] += int((~mask).sum())
            users = matrices[0][
                [i for i, u in enumerate(built[0].units) if u.body.role == Role.USER]
            ]
            if original_users is None:
                original_users = users
            else:
                np.testing.assert_array_equal(users, original_users)
            if context == Context.SELECTED:
                context_changed += sum(bool(u.prompt.text) for u in built[0].units)
    report = AblationReport.model_validate_json(
        (parent / "turn-three-factor-ablation-20261006/development.json").read_bytes()
    )
    specimens = tuple(
        min(
            (
                e
                for e in entries
                if (sum(bool(t.prompts) for t in e.captured.turns) >= 8) == long
                and sum(
                    u.input.role == Role.ASSISTANT
                    for u in source(e, parent, report)[0].units
                )
                >= 2
            ),
            key=lambda e: len(source(e, parent, report)[0].units),
        )
        for long in (False, True)
    )
    root = output.parent / "isolation"
    root.mkdir(mode=0o700, exist_ok=False)
    encoder = Encoder(model)
    checks = 0
    with threadpool_limits(limits=1):
        for entry in specimens:
            variants = []
            for case in ("alone", "other-first", "mutated-other"):
                directory = root / f"{entry.captured.index}-{case}"
                directory.mkdir(mode=0o700)
                store = Store(directory / "vectors.sqlite")
                try:
                    if case == "other-first":
                        other = next(e for e in specimens if e != entry)
                        build(
                            other, parent, report, directory, contract, encoder, store
                        )
                    elif case == "mutated-other":
                        store.vectors(
                            (
                                "Unrelated changed history about genomic cohorts.",
                                "Different reordered history about CSS layout.",
                            ),
                            encoder,
                            lambda *_: None,
                            singleton=True,
                        )
                    build(entry, parent, report, directory, contract, encoder, store)
                    variants.append(directory)
                finally:
                    store.close()
            for setting in contract.settings:
                filename = name(entry.captured.index, setting)
                reference = Artifact.model_validate_json(
                    (variants[0] / (filename + ".json")).read_bytes()
                )
                vectors, owners = matrix(
                    (variants[0] / (filename + ".npz")).read_bytes()
                )
                groups = fit(vectors, owners)
                for directory in variants[1:]:
                    current = Artifact.model_validate_json(
                        (directory / (filename + ".json")).read_bytes()
                    )
                    current_vectors, current_owners = matrix(
                        (directory / (filename + ".npz")).read_bytes()
                    )
                    if current.units != reference.units or current_owners != owners:
                        raise ValueError("surrounding input changed selected units")
                    np.testing.assert_array_equal(current_vectors, vectors)
                    if fit(current_vectors, current_owners) != groups:
                        raise ValueError(
                            "surrounding input changed cluster memberships"
                        )
                    checks += 1
    result = Audit(
        histories=len(entries),
        artifacts=len(entries) * len(contract.settings),
        weighted_points=weighted,
        changed_weighted_vectors=changed,
        unchanged_unweighted_points=unchanged,
        changed_context_points=context_changed,
        isolation_histories=tuple(e.captured.index for e in specimens),
        isolation_settings_checked=checks,
    )
    output.write_bytes(orjson.dumps(result.model_dump(mode="json")))
    click.echo(
        f"Audited {result.artifacts} artifacts and {checks} isolation comparisons"
    )


if __name__ == "__main__":
    main()
