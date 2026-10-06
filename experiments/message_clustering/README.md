# Within-thread clustering experiment

This isolated Python environment implements the candidate algorithms from
[the preregistered evaluation](../../docs/message-clustering-evaluation.md).
It is not imported by Mash, does not change ingestion or search, and uses no
generative model inference. Turn-representation experiments use explicitly
installed local embedding assets. Dependencies are pinned in `uv.lock`.

The [turn-clustering literature review](../../docs/turn-clustering-prior-art.md)
records the rationale and expanded sampling procedure. `snapshot_node.py`
captures a read-only research copy of a node's SQLite database and native files;
`turn_study_corpus.py --threads 200 --min-prompted-turns 2
--confirmation-per-provider 20 --snapshot /private/capture` combines that node
with the database supplied by `--database`. Also supply both old `--previous`
corpora and a private `--output-dir`. Selection fails if it cannot meet the
independent-thread target. Captures contain sensitive raw history; do not commit
or publish them. Sampling does not change production Mash.

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

## Turn embeddings and repeated algorithm screen

See [turn-embedding-experiment.md](../../docs/turn-embedding-experiment.md).
`turns.py` extracts prompt bundles and answer segments, ending a turn at the next
user reply after any response. Branch boundaries take precedence. `local_encoder.py`
loads installed quantized MiniLM assets with explicit token limits; it cannot
download models. `turn_artifact.py` stores source-linked recipes separately from
message embeddings in private SQLite, with transactional publication and
content-addressed vector reuse. Production schema and search behavior are unchanged.

```bash
env OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  uv run python run_turns.py --corpus /path/to/new-corpus.json \
  --references /path/to/new-labels.toml --snapshot /path/to/installed/snapshot \
  --output-dir /home/james/scratch/mash-artifacts/new-turn-run
```

Runs only development: four representations at three average-link cuts, then
the fixed 27 algorithm settings on the representation selected by cross-turn
macro F1. Report turn-bundling, stored-message and same-runtime message controls,
original-source scores, input coverage, truncation and build/reuse costs.
Artifacts and reports stay private. Completed reports cannot be overwritten;
interrupted builds can reuse vectors without replacing a published recipe until
rebuilding succeeds. Pooled control vectors are rebuilt from the unchanged
corpus rather than stored as independently inferred cache entries.

Use `summarize_turns.py --report ... --corpus ... --references ... --output ...`
for original context/window baselines and micro metrics including negative-only
threads. `fit(..., bundle_overlay=True)` exposes the separate post-hoc diagnostic
that adds within-turn bundles to the selected clusters. It is never used for
registered candidate selection. The completed comparison found modest combined
view improvement but insufficient precision/baseline gain; reserved data stayed
unjudged and unscored. See the protocol/results document for all limitations.

## Complete ancestry and audited replication

Visible embedded messages can point to omitted tool/reasoning records.
`turn_ancestry.py` captures complete canonical histories and native files, then
bridges those parents while retaining roots and branches. Omitted assistant
activity also closes a prompt bundle before the next user reply.
`audit_turns.py` checks provenance/coverage and selects balanced boundary cases.
See [turn-ancestry-audit.md](../../docs/turn-ancestry-audit.md).

```bash
uv run python turn_ancestry.py --database /path/to/mash.sqlite3 \
  --corpus /path/to/new-corpus.json \
  --output-dir /home/james/scratch/mash-artifacts/new-ancestry
uv run python audit_turns.py \
  --capture /home/james/scratch/mash-artifacts/new-ancestry/capture.json \
  --output /home/james/scratch/mash-artifacts/new-ancestry/audit.json
# Inspect the selected private boundary cases before repeating the screen.
env OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  uv run python run_turns.py --corpus /path/to/new-corpus.json \
  --references /path/to/new-labels.toml --snapshot /path/to/installed/snapshot \
  --ancestry /home/james/scratch/mash-artifacts/new-ancestry/capture.json \
  --output-dir /home/james/scratch/mash-artifacts/new-ancestry-run
uv run python summarize_turns.py --corpus /path/to/new-corpus.json \
  --references /path/to/new-labels.toml \
  --report /home/james/scratch/mash-artifacts/new-ancestry-run/report.json \
  --ancestry /home/james/scratch/mash-artifacts/new-ancestry/capture.json \
  --output /home/james/scratch/mash-artifacts/new-ancestry-run/summary.json
```

The optional ancestry input must cover exactly the frozen development cohort;
both runner and summarizer recheck source identity and native capture hashes.
The summarizer requires the capture digest recorded by the runner so its
cross-turn baselines use the same boundaries. Original message vectors and
reserved data remain unchanged.

## Registered 200-thread study

`blind_turn_pairs.py` freezes opaque pair packets before predictions, and
`study_annotations.py` validates full coverage and literal evidence while
exposing only development judgments and a confirmation-adequacy boolean.
Review labels remain independent diagnostics. See
[the frozen protocol](../../docs/turn-clustering-study.md).

`study_artifacts.py` builds five views without reading annotations. It stores
per-thread point matrices with turn owners, source-preserving artifact manifests
and inference/cache costs. Optional `--cache` reads an existing vector store
without modifying it; timings with that option are cache-assisted.

`study_scoring.py` implements the fixed 60 candidates, distinct-turn retention,
passage-to-turn membership unions, lexical/chronological controls, micro counts
including negative-only histories, and the development stopping rules. It is
the tested scoring core. `run_study_development.py` checks completeness,
provenance and registered label adequacy before scoring the development grid.
It hashes the consumed matrices and freezes one development choice in an
exclusive output file, with paired thread-bootstrap uncertainty. Lexical and
chronological controls share TF-IDF construction and report amortized runtime.
Confirmation evaluation and the final decision remain pending.
Nothing here changes production search or message embeddings.

`enrich_turn_pairs.py` is the single registered remedy for insufficient label
counts. It requires complete initial judgments, preserves the initial manifest,
and samples at most four unsampled pairs per history using only identities and
position. It refuses a second batch. The development reader keeps confirmation
closed while annotation is incomplete. `confirm_turn_study.py` unseals only
after a development selection, checks frozen annotation and matrix hashes, and
evaluates only that choice and its registered baseline. It reports quality
acceptance separately from the remaining performance/production checks.

`study_analysis.py` aggregates provider, node, length and sampling-stratum/batch
counts, preserving negative-only histories and primary-only diagnostics.
`summarize_turn_study.py` checks complete configuration/thread coverage and
frozen annotation provenance before exporting those breakdowns, weighted
assignment coverage, group sizes and per-view construction costs. Optional
confirmation input must match the frozen development report. This command
does not fit new configurations or change the selected method.

```bash
uv run python summarize_turn_study.py \
  --development /path/to/frozen-development.json \
  --annotations /path/to/blind-packets \
  --costs /path/to/artifact-build/costs.jsonl \
  --output /home/james/scratch/mash-artifacts/new-study-analysis.json
```

Deterministic turn cleanup/extractive selection is tracked separately in bead
`mash-jfn`; it does not alter this study's registered five views or thresholds.

`audit_turn_drift.py` is a development-only post-result diagnostic. For two
fixed configurations it refits prefixes ending at each pair's later turn while
holding vectors/order fixed. It reports decision changes separately from
pairwise cosine discrimination. It does not tune thresholds, select a method
or inspect confirmation judgments.

The experimental vector cache checkpoints up to 128 new vectors per durable
SQLite transaction. Inference still uses single inputs when requested, or the
original batches of eight; transaction size must not change model batching
because quantized outputs can depend on batch companions. Failed transactions
roll back completely and may require recomputing up to 128 vectors on restart.
Completed checkpoints remain reusable. This reduces small commits under disk
contention without weakening SQLite durability or changing cache identities.
Already-running experiments retain their loaded implementation; frozen studies
must preserve their pinned builder source when resuming.
