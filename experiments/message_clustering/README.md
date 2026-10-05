# Within-thread clustering experiment

This isolated Python environment implements the candidate algorithms from
[the preregistered evaluation](../../docs/message-clustering-evaluation.md).
It is not imported by Mash, does not change ingestion or search, and uses no LLM
or embedding inference. Dependencies are pinned in `uv.lock`.

From this directory:

```bash
uv sync --locked
uv run python -m unittest discover -v
uv run ruff check .
uv run ruff format --check .
uv run ty check
```

`clustering.py` accepts one thread's unique, ordered message projections as a
float64 matrix. It fits leader, BIRCH, average-link and HDBSCAN partitions,
retains groups with at least two substantive projections, and computes overlapping
memberships and source-message representatives. `OnlineLeader.append` preserves
centroid sums between insertions. BIRCH receives individual messages with
`partial_fit`; final assignments use all messages, not just its last batch.
Average-link and HDBSCAN are offline candidates. Edits/deletions require
affected-thread replay. Offline results could be cached by thread revision and
recomputed on request after a change; this cache is not implemented yet.

The signed-byte decoder validates Rust's stored message-vector dimensions and
norms. The caller must isolate embedding generations, preserve missing-vector
counts, deduplicate prepared projections, and map indices back to every original
message. `preparation.py` deduplicates prepared projections and maps results back
to every source message. Its context view uses a fixed eligible parent and never
reads pair judgments to choose vectors or support masks.

`corpus.py` reads one SQLite transaction, verifies the default dialogue recipe,
vector metadata and per-thread generation, and preserves source rows and counts.
It checks Codex fork/parent metadata and Claude UUID/parent/session links,
conservatively joins copies by projection overlap, and selects independent
components across two providers and three length strata. Output is sensitive
typed JSON, written exclusively under the private artifact directory with mode
0600; overwrites are refused. Never commit the output file.

```bash
uv run python corpus.py --database /path/to/mash.sqlite3 \
  --output /home/james/scratch/mash-artifacts/message-clustering-20261005/corpus.json
```

`evaluation.py` scores only supplied pair judgments, with explicit related,
unrelated and ambiguous verdicts. Ambiguous pairs do not enter precision/recall;
unjudged pairs are never generated as negatives. Duplicate, self and out-of-range
references fail. `references.py` validates private TOML judgments against the
frozen corpus. `run_evaluation.py` reports macro scores, coverage and strata;
`frozen_settings.py` selects settings from development reports. Held-out scoring
requires those settings and matching corpus/reference hashes.

Current tests are synthetic contracts for codecs, deterministic grouping,
noncontiguous returning subjects, overlapping memberships, abstention, incremental
leader replay and metric boundaries. They do not establish real-session quality.
The real comparison is complete: twenty-four development configurations and one
held-out pass. **None passes the promotion gates.** Context-aware average-link
is the strongest research candidate, but its 0.774 F1 versus the chronological
baseline's 0.769 is insufficient. Offline rebuild cost is acceptable in the
measured fixture; no production algorithm or cache is implemented.
See the evaluation document for results, limitations and the stop decision.
Keep private histories, vectors, judgments
and result artifacts under `/home/james/scratch/mash-artifacts`, outside Git.

## Reproduction commands

Use existing private inputs and fresh output names (commands refuse overwrites).
Do not repeat held-out scoring while adjusting settings.

```bash
uv run python run_evaluation.py --corpus /path/to/corpus.json \
  --references /path/to/labels.toml --split development --context \
  --output /home/james/scratch/mash-artifacts/development-context.json
uv run python frozen_settings.py --report /path/to/development-raw.json \
  --report /path/to/development-context.json \
  --output /home/james/scratch/mash-artifacts/frozen-settings.json
uv run python run_evaluation.py --corpus /path/to/corpus.json \
  --references /path/to/labels.toml --split held_out --context \
  --settings /path/to/frozen-settings.json \
  --output /home/james/scratch/mash-artifacts/held-out.json
uv run python benchmark.py --fixture /path/to/grouping-fixture.npz \
  --settings /path/to/frozen-settings.json --method average_link --messages real \
  --output /home/james/scratch/mash-artifacts/benchmark-average-link.json
```

The timing fixture is an NPZ with `vectors` (prepared float64 matrix) and
`substantive` (support mask), produced from a frozen development thread with
`preparation.prepare`. Set `OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1
MKL_NUM_THREADS=1` before running; numerical thread counts are also bounded
inside the runner. `benchmark.py` reports ten warm rebuilds, three controlled
updates per kind, peak RSS and cold imports separately. Its 5,000-unit scaling
case is synthetic. Private qualitative group judgments supplement the reports.

## Additional algorithms and larger cohort

The separate follow-up is specified in
[message-clustering-alternatives.md](../../docs/message-clustering-alternatives.md).
`expanded_corpus.py` freezes 72 fresh components, excluding the first study:
48 development and 24 reserved. `alternatives.py` defines fifteen fixed settings
for complete-link, DP-means, OPTICS, spectral and affinity propagation.
`screen_alternatives.py` runs only development, using isolated Linux processes
with deadlines and explicit nonconvergence. No dependencies were added.

```bash
uv run python expanded_corpus.py --database /path/to/mash.sqlite3 \
  --previous /path/to/first-study-corpus.json \
  --output /home/james/scratch/mash-artifacts/new-corpus.json
# Freeze diagnostic references before running the screen.
env OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  uv run python screen_alternatives.py --corpus /path/to/new-corpus.json \
  --references /path/to/new-labels.toml \
  --output /home/james/scratch/mash-artifacts/new-screen.json
```

No candidate passed development screening on the expanded cohort; reserved data
remain unjudged/unscored. The living LaTeX paper is
[`docs/paper/message-clustering.tex`](../../docs/paper/message-clustering.tex).
