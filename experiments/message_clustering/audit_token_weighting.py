"""Artifact integrity, real singleton parity and unrelated-history isolation."""

import os
from pathlib import Path

import click
import numpy as np
import orjson
from threadpoolctl import threadpool_limits

from ablation_evidence import fit, fuse
from build_input_followup import Built
from build_local_representation import source
from build_token_weighting import Artifact, Contract, digest
from corpus import Record
from local_encoder import Encoder
from run_turn_ablation import AblationReport
from temporal_smoothing import Setting, lineages, smooth
from token_weighting import Frequencies, Policy, encode_policies
from turn_artifact import generation
from turn_study_corpus import Cohort, Phase


class Movement(Record):
    vectors: int
    changed: int
    mean_cosine: float
    minimum_cosine: float


class Audit(Record):
    histories: int
    input_vectors: int
    tokens: int
    weighted_tokens: dict[Policy, int]
    movement: dict[Policy, Movement]
    build_seconds: float
    build_p95_seconds: float
    contract_sha256: str
    parity_probes: int
    isolation_probes: int
    stored_vector_probes: int
    membership_probes: int


@click.command()
@click.option("--parent", type=click.Path(path_type=Path, exists=True), required=True)
@click.option(
    "--artifacts", type=click.Path(path_type=Path, exists=True), required=True
)
@click.option("--model", type=click.Path(path_type=Path, exists=True), required=True)
@click.option("--output", type=click.Path(path_type=Path), required=True)
def main(parent: Path, artifacts: Path, model: Path, output: Path) -> None:
    os.umask(0o077)
    contract_raw = (artifacts / "contract.json").read_bytes()
    contract = Contract.model_validate_json(contract_raw)
    for name, expected in contract.code_sha256.items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError("build code changed")
    cohort_raw = (
        parent / "turn-study-fleet-verified-20261005/cohort.json"
    ).read_bytes()
    if digest(cohort_raw) != contract.cohort_sha256:
        raise ValueError("cohort changed")
    cohort = Cohort.model_validate_json(cohort_raw)
    entries = tuple(e for e in cohort.entries if e.phase == Phase.DEVELOPMENT)
    totals = dict.fromkeys(Policy, 0)
    cosines: dict[Policy, list[np.ndarray]] = {p: [] for p in Policy}
    changed = dict.fromkeys(Policy, 0)
    records = []
    for entry in entries:
        path = artifacts / f"{entry.captured.index}.json"
        record = Artifact.model_validate_json(path.read_bytes())
        if (
            record.contract_sha256 != digest(contract_raw)
            or record.source_sha256
            != digest(orjson.dumps(entry.captured.model_dump(mode="json")))
            or record.matrix_sha256 != digest(path.with_suffix(".npz").read_bytes())
        ):
            raise ValueError("artifact provenance differs")
        records.append(record)
        with np.load(path.with_suffix(".npz"), allow_pickle=False) as archive:
            for policy in Policy:
                totals[policy] += record.weighted_tokens[policy]
                for prefix in ("pooled", "chunks"):
                    control = archive[f"{prefix}_{Policy.UNIFORM}"]
                    treatment = archive[f"{prefix}_{policy}"]
                    cosines[policy].append(np.sum(control * treatment, axis=1))
                    changed[policy] += int(
                        np.count_nonzero(np.any(control != treatment, axis=1))
                    )
    encoder = Encoder(model)
    if generation(encoder) != contract.encoder_generation:
        raise ValueError("installed encoder differs")
    report = AblationReport.model_validate_json(
        (parent / "turn-three-factor-ablation-20261006/development.json").read_bytes()
    )
    parity = isolation = stored = memberships = 0
    for index in (91, 109):
        entry = next(e for e in entries if e.captured.index == index)
        structured, _, _, _ = source(entry, parent, report)
        pooled = Built.model_validate_json(
            (
                parent
                / "turn-input-followup-20261005/artifacts"
                / f"{index}-context_whole.json"
            ).read_bytes()
        )
        own = Frequencies.from_messages(
            tuple(m.projection for m in entry.captured.thread.messages)
        )
        requests = tuple(
            (pooled.inputs[i].content.text, pooled.inputs[i].target)
            for i in (0, len(pooled.inputs) // 2, len(pooled.inputs) - 1)
        ) + tuple(
            (structured.units[i].input.content.text, None)
            for i in (0, len(structured.units) // 2, len(structured.units) - 1)
        )
        first = [
            encode_policies(encoder, text, own, selection)[0]
            for text, selection in requests
        ]
        # Perturb both inference order and unrelated frequency estimation.
        encode_policies(
            encoder,
            "unrelated prose the database",
            Frequencies.from_messages(("the " * 100,)),
        )
        for position in reversed(range(len(requests))):
            text, selection = requests[position]
            again, _ = encode_policies(encoder, text, own, selection)
            plain = encoder.encode_selected((text,), (selection,))[0]
            np.testing.assert_array_equal(plain, first[position][Policy.UNIFORM])
            parity += 1
            for policy in Policy:
                np.testing.assert_array_equal(first[position][policy], again[policy])
                isolation += 1
                if position >= 3:
                    source_index = (
                        0,
                        len(structured.units) // 2,
                        len(structured.units) - 1,
                    )[position - 3]
                    with np.load(
                        artifacts / f"{index}.npz", allow_pickle=False
                    ) as archive:
                        np.testing.assert_array_equal(
                            archive[f"chunks_{policy}"][source_index], again[policy]
                        )
                    stored += 1
        with np.load(artifacts / f"{index}.npz", allow_pickle=False) as archive:
            a_owners = tuple(archive["pooled_owners"].tolist())
            b_owners = tuple(archive["chunk_owners"].tolist())
            with threadpool_limits(limits=1):
                for policy in Policy:
                    a = smooth(
                        archive[f"pooled_{policy}"],
                        a_owners,
                        lineages(entry.captured.thread, entry.captured.turns),
                        Setting(alpha=0.10, window=4),
                    )
                    b = archive[f"chunks_{policy}"]
                    before = fuse(
                        fit(a, a_owners),
                        fit(b, b_owners),
                        (0.0,) * len(b_owners),
                        False,
                    )
                    fit(np.eye(384)[:3], ("unrelated-a", "unrelated-b", "unrelated-c"))
                    after = fuse(
                        fit(a, a_owners),
                        fit(b, b_owners),
                        (0.0,) * len(b_owners),
                        False,
                    )
                    if before != after:
                        raise ValueError("memberships depend on unrelated fit")
                    memberships += 1
    movement = {}
    for policy in Policy:
        values = np.concatenate(cosines[policy])
        movement[policy] = Movement(
            vectors=len(values),
            changed=changed[policy],
            mean_cosine=float(values.mean()),
            minimum_cosine=float(values.min()),
        )
    audit = Audit(
        histories=len(records),
        input_vectors=sum(r.inferred_inputs for r in records),
        tokens=sum(r.tokens for r in records),
        weighted_tokens=totals,
        movement=movement,
        build_seconds=sum(r.seconds for r in records),
        build_p95_seconds=float(np.quantile([r.seconds for r in records], 0.95)),
        contract_sha256=digest(contract_raw),
        parity_probes=parity,
        isolation_probes=isolation,
        stored_vector_probes=stored,
        membership_probes=memberships,
    )
    output.write_bytes(orjson.dumps(audit.model_dump(mode="json")))
    click.echo(
        f"Validated {audit.histories} histories and "
        f"{audit.isolation_probes} isolation comparisons"
    )


if __name__ == "__main__":
    main()
