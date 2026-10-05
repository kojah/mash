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
message. Fitting-time deduplication remains pending.

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
references fail. Reports still need per-thread macro aggregation, coverage,
stratification and qualitative group review before any acceptance claim.

Current tests are synthetic contracts for codecs, deterministic grouping,
noncontiguous returning subjects, overlapping memberships, abstention, incremental
leader replay and metric boundaries. They do not establish real-session quality.
The real corpus is frozen; blind references, performance/update evaluation and final
algorithm decision remain pending. Keep private histories, vectors, judgments
and result artifacts under `/home/james/scratch/mash-artifacts`, outside Git.
