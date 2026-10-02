# Stage comparison boundary

`stage_parity.py` is a standard-library development tool. Its synthetic
adversarial tests run in CI without science packages, private histories, model
downloads or credentials. This implements the **comparison boundary only**:
there is no selected topic algorithm, Python reference runner, Rust runner or
complete algorithm-specific fixture/schema gate yet. See [PORT_CONTRACT.md](PORT_CONTRACT.md).

The policy is a UTF-8 JSON file with exactly these fields:

```json
{
  "schema_version": 1,
  "stages": [
    {"name": "units", "diagnostics": {}},
    {"name": "clustering", "diagnostics": {
      "cosine_scores": {"absolute": 1e-9, "relative": 0.0}
    }}
  ]
}
```

Available ordered stage names are `units`, `context`, `embeddings`, `clustering`,
`labels`, `tags`, `lifecycle`. The policy may describe a nonempty subset for a
stage-specific fixture; duplicates/reordering fail. A subset passing comparison
does not establish complete pipeline coverage. Before the selected port ships,
its fixture manifest must require all applicable stages and validate their
specific typed schemas, dimensions and complete source coverage.

Both runners must emit this envelope, replacing hashes with SHA256 of the exact
policy and fixture bytes. No normalization of those files occurs:

```json
{
  "schema_version": 1,
  "contract_sha256": "<policy SHA256>",
  "fixture_sha256": "<fixture SHA256>",
  "stages": [
    {"name": "units", "decisions": {
      "sources": [{"id": "source-a", "start": 0, "end": 2}]
    }, "diagnostics": {}},
    {"name": "clustering", "decisions": {
      "memberships": [["source-a", "source-b"]]
    }, "diagnostics": {"cosine_scores": [0.5]}}
  ]
}
```

`decisions` contains exact discrete objects: strings, JSON-range integers,
booleans, nulls and ordered arrays/objects. Object field names must match
`[a-z][a-z0-9_]{0,63}`; represent source IDs as record values, not dynamic keys.
Floats belong in named `diagnostics`, never in the discrete decision tree.
Use canonical stable source membership arrays and canonical parent-edge records,
not implementation-specific cluster numbers. Runners own that canonicalization;
the comparator deliberately neither sorts arrays nor relabels trees to conceal
a changed membership, rank, tie decision or parent relationship.

Diagnostics are explicitly named flat arrays of finite JSON floats. Every
diagnostic needs a policy entry, even for zero tolerance. Extra/missing fields,
integer/bool diagnostic entries, shape drift, nonfinite values and numeric
comparison overflow fail. Compare each component using
`abs(candidate-reference) <= absolute + relative * abs(reference)`.
Tolerance comes from the caller's pinned policy, never the candidate output.
Signed zero compares numerically; any required bitwise distinction needs a
separate exact discrete representation in the selected algorithm contract.

Run:

```bash
python3 research/topics/stage_parity.py \
  --contract POLICY.json --fixture FIXTURE.json \
  --reference PYTHON_OUTPUT.json --candidate RUST_OUTPUT.json
```

The tool verifies both provenance headers, then visits stages in contract order,
validating and comparing each before the next. It exits 1 with the first
stage/path failure; reports omit expected/actual source payload values. Numeric
success reports contain the maximum absolute drift and component count. JSON
parsing rejects duplicate keys, nonfinite constants/exponents, out-of-range
integers, malformed encoding and unsupported schema versions. Decision nesting
is bounded to 128 path components. This is an offline trusted-fixture tool, not
an HTTP input boundary or streaming large-corpus comparator.

The tests mutate provenance, stage coverage/order, JSON types, memberships,
ancestry, numeric shapes and near-threshold values, plus exercise the actual CLI
and ensure failures do not print source values. A tiny tolerated score drift
never excuses a changed membership. Matching two malformed or incomplete
algorithm outputs can still pass this generic envelope: the selected runner's
typed schema/source/model/invariant validation remains mandatory.

This checker is neither inference fidelity nor semantic quality evidence.
The complete Rust/Python runner integration, generated algorithm cases,
supported-platform inference and defined edit/delete/restart fixtures remain
tracked by `mash-c0d.19.2.26`.
