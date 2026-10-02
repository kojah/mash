# Executable scientific reference environment

The long-lived scientific environment used for the frozen controls is CPython
3.11.15 with NumPy2.4.6, SciPy1.17.1 and sklearn1.9.1. The broader project
`pyproject.toml`/`uv.lock` previously selected NumPy2.5.3/SciPy1.18.1. Those
are separate dependency selections; `uv sync` is not evidence of reproducing
historical captures. Preserve old captures and state the environment used.

`science-requirements.in` records all 43 observed distribution versions,
including the optional graph, HDBSCAN and background-frequency dependencies.
`science-requirements.txt` freezes allowed distribution archive hashes. These
files describe the current executable scientific reference; they do not assert
that all earlier experiments used every optional package.

With `uv` and CPython3.11.15 already installed, create a fresh environment outside
the repository:

```sh
python3 research/topics/science_env.py --create /absolute/outside/repo/reference
```

The initializer refuses existing destinations and paths inside the repository.
It never clears or updates an existing environment, automatically downloads a
Python interpreter, downloads model weights or runs inference. It installs the
hash-locked packages into `reference/venv` using copies, verifies the exact Python
implementation/version and complete package set, and writes `manifest.json` and
`setup.log` beside the environment. Failed setups retain diagnostics. Install
CPython3.11.15 explicitly before retrying into a new destination if unavailable.
The executable layout is currently for Linux; cross-platform creation and
numeric equivalence are separate checks.

Verify any reference interpreter without changing it:

```sh
python3 research/topics/science_env.py --verify /absolute/reference/venv/bin/python
```

Unexpected/missing packages, changed versions or duplicate canonical distribution
names fail. The manifest includes package/Python/platform metadata and pin, hash
lock and initializer SHA256 identities. Archive hashes plus version equality do
not prove identical native builds, BLAS, hardware or model assets. Require the
actual frozen experiment comparison too; this environment is not a Rust port.

Run research with that interpreter and explicit thread/temp settings, for example:

```sh
env TMPDIR=/absolute/durable/tmp PYTHONDONTWRITEBYTECODE=1 \
  OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /absolute/reference/venv/bin/python -m unittest discover \
  -s research/topics -p 'test_*.py'
```

Supply the documented external author-reference paths for LINKs/FISHDBC tests;
an unsupplied/skipped author fixture is not parity evidence. Scientific runs
still need immutable input/model identities, code pins, insertion/seed settings,
exact non-runtime repeats, independent stage audits and complete-ingestion/quality
gates. A matching environment alone establishes none of those results.

To intentionally regenerate archive hashes from these exact versions:

```sh
uv pip compile research/topics/science-requirements.in --generate-hashes \
  --no-annotate --no-header --python-version 3.11.15 --no-config \
  --default-index https://pypi.org/simple \
  --output-file research/topics/science-requirements.txt
```

Changes to pins or allowed archives require a recorded reason, fresh captures
and comparisons; do not overwrite old results or silently relax numerical
checks. Standard-library environment guard tests run in CI. CI tests the
guardrail, not private historical corpora or successful full environment creation.
