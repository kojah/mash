# Fresh scientific environment reproduces the frozen flat-topic controls

## Problem and procedure

The long-lived reference uses CPython3.11.15/NumPy2.4.6/SciPy1.17.1/
sklearn1.9.1, while the pre-existing general research lock selected
NumPy2.5.3/SciPy1.18.1. A generic `uv sync` did not describe those experiments.
Preserve both dependency selections and all old captures; give the scientific
reference its own explicit executable environment.

Snapshot all 43 actually installed distribution versions into
`research/topics/science-requirements.in`; generate archive-hashed
`science-requirements.txt` using the installed uv compiler for Python3.11.15.
The standard-library `science_env.py` initializer creates a fresh directory
outside Git, disables automatic Python downloads, installs hash-verified packages
using copies, verifies CPython/version and the complete canonical package set,
then records manifest and pin/lock/initializer hashes. It rejects existing
destinations and repository paths. Failed attempts retain diagnostics.
The original environment is verified without modification.

Fresh environment: private `science-reference-env-20261002/venv`. Both original
and fresh environments verify the exact same 43 package versions. Input pins
SHA256 `370e44b1344bc5cf00ab3c456b3a1553710000189e76b945baea1759f1429972`;
archive lock SHA256 `297840e11c18695a53684192d940b8d8908f48bd342256f07d40ea33d6e7136b`.
Store original and fresh manifests privately. Version/archive agreement alone
does not prove native builds, BLAS, hardware or every historical experiment;
test actual captured behavior next.

Under the fresh interpreter, execute the complete four-policy source control
(whole-thread, source coverage, phatic coverage, English-background coverage)
on the same eight-thread/68-source reviewed data and 128-thread/1,850-source
cohort. Use the unchanged original data/code/baseline inputs, one OMP/OpenBLAS/
MKL worker and private durable TMPDIR. Compare every non-runtime field against
the original `phrase-background-first-20261002.json` capture. No algorithm,
scoring/threshold change, inference, model download, LLM, source reselection or
paid call occurs. Originals remain untouched.

## Results and verification

Every non-runtime field matches exactly, including all eight snapshots,
background source/asset provenance, counts, ranks, scores, source memberships,
review metrics and producer duplicate/delete/restart/reinsert/reverse checks.
This verifies a representative reference replay, not all past methods/platforms.
The previously mixed/negative topic-quality results remain mixed/negative.

The fresh interpreter also runs the no-producer-import direct-SQLite/raw-frequency
audit: all 3,379 selected scalar ranks/scores agree exactly and maximum reviewed
support F1 error is 1.11e-16. Static stoplist, frequency assets, declared formulas
and original review are shared dependencies. Independent native parsing,
lifecycle replay, semantic gold and selected Rust parity remain open.

All 661 research tests pass in 4.485s under the new environment with the external
LINKs/FISHDBC author-reference paths supplied. Three new standard-library guards
run in CI: exact pin syntax/canonical collisions; Python/package-version/extra
distribution drift; refusal of repository/existing destinations while preserving
existing contents. They pass independently in 0.004s. Workflow YAML parses.
CI runs those guards, not this private-corpus replay or full package installation.

The port contract now explicitly reflects the flat/multi-label milestone:
scoped thread memberships and original evidence are required, semantic ancestry
and inherited tree tags are optional future research. The scientific environment
is referenced as a prerequisite rather than a completed Rust parity gate.

## Runtime, scope and artifacts

Whole four-arm/cross-scope/lifecycle replay takes 91.001s wall, 90.055s CPU after
reviewed loading and before serialization/comparison/final asset recheck.
Frequency loading is 482.73ms, broad refresh times 3.096/3.241/3.847/3.534s in
the arm order above. These observational timings include multiple lifecycle
refreshes and broad loading; imports/initial reviewed loading are excluded.
They are not a performance comparison, isolated worker or full-ingestion gate.

Sampled target RSS/status HWM is 234,428 KiB, PSS224,230 KiB, pre-serialization
rusage230,084 KiB; 3,416 samples, one vanished read, maximum gap48.57ms. Sample
peaks are lower bounds, status approximate, observer intrusive, children/observer
excluded and launcher history may affect rusage. Do not subtract counters or
infer complete-ingestion overhead.

Replay SHA256 `8f392fdbe90c3debcc4abe17ebf99611b4c1901438e13e89aa18158be0d87507`.
Artifacts are `science-reference-control-20261002.json`, memory sidecar, replay/
audit/test logs, original manifest and initializer directory manifest/setup log
under mash-artifacts. No raw source, labels or credentials enter Git.
Creation/replay evidence is from this Linux machine; Mac/Windows/native builds,
all historical controls, full-ingestion/quality and Rust parity are unproven.
See [executable workflow](../research/topics/SCIENCE_ENVIRONMENT.md).
