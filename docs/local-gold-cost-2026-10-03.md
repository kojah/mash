# Local gold representation costs, 2026-10-03

This bounded real-session measurement covers representation switching and fresh
CLI processes. It does not satisfy the full shared/fleet or held-out quality
acceptance gates. Default build and ranking behavior remain unchanged.

## Procedure and setup

`tools/benchmark-gold.py` selects stable copied JSONL files from Codex and
Claude Code on one Linux node. Within each provider, paths are ordered by a
fixed SHA-256 sampling key, selecting at most four files in each size interval:
1–64 KiB, 64–512 KiB, 512 KiB–2 MiB, and 2–8 MiB. Stable inode, size and
mtime are checked around copying. This size-stratified convenience sample is
not a random sample of messages or users; large sessions are excluded.

The run selected 12 Codex and 16 Claude files (45,796,982 source bytes), producing
25 canonical threads and 6,137 messages. It used all history in those files,
not a 90-day cutoff. Source ingestion produced a complete silver certificate
with no rejected sources or malformed records.

The initial configuration was lexical-only. The harness then deleted its own
source copies and performed three source-free gold replacements: dense,
lexical-only, dense. All ten retained canonical/provenance tables were hashed
before and after every replacement and after the search matrix. Their common
SHA-256 was `dd22a1958591301fc20038a6a2714d6873a332eb029f33d81645202ffb735935`.
Every replacement reported zero scanned provider files. Lexical-only operations
reported zero model inferences. The live provider files and installed index
were untouched.

The node exposes 16 x86-64 CPUs (AMD EPYC 9645) and about 62.8 GiB RAM, running
Python 3.13.5. The binary was built from `f6207f7` with
`cargo build-clif --release --locked -j 2`, incremental compilation disabled.
The alias's Cranelift override applies to **dev**, so this release uses optimized
LLVM compilation. Its SHA-256 was
`426f68fe43b8da12a58f5942c88cb8d915811991ee6a8f4a685c8a81ffd059a1`.
Installed local models were quantized AllMiniLML6V2Q (384 dimensions) and
`jinaai/jina-reranker-v1-turbo-en`; no downloads or remote inference occurred.

## Construction and storage

| Operation | Wall seconds | Child CPU seconds | Sampled RSS maximum, MiB | Occupied SQLite pages, MiB | Embedding rows |
| --- | ---: | ---: | ---: | ---: | ---: |
| Source ingestion + lexical | 0.441 | 0.567 | 47.3 | 19.582 | 0 |
| Replace with lexical + dense | 8.052 | 26.166 | 1,383.4 | 20.555 | 1,061 |
| Replace with lexical-only | 0.100 | 0.067 | 21.3 | 19.586 | 0 |
| Replace with lexical + dense again | 7.500 | 24.241 | 1,389.0 | 20.555 | 1,061 |

Both dense builds reported 1,045 model inferences. Canonical message count is
not inference count: visibility, retrieval eligibility, reuse and chunking
affect representation construction. The embedding table, its primary-key index
and semantic chunks occupied 0.980 MiB with dense enabled, versus 0.012 MiB of
empty table/index pages with lexical-only.

Occupied pages here means `(page_count - freelist_count) * page_size`, including
all silver and gold structures. It is not unique payload size. SQLite's logical
file allocation was 19.613, 20.555, 21.402 and 21.402 MiB across the four stages;
the last two included 1.816 and 0.848 MiB of reusable free pages respectively.
Replacing dense gold therefore removed its rows without shrinking the database
file. Captured WAL sizes were 0, 2,995,272, 5,290,112 and 0 bytes. WAL checkpoint
timing and page reuse make file-size snapshots unsuitable for claiming that a
lexical-only replacement increases retained content. No vacuum was performed.

Memory was sampled every 20 ms only when `/proc/PID/exe` matched the benchmark
binary. RSS/PSS maxima are lower bounds; initial ingestion's sampled VmHWM was
51.8 MiB, higher than its sampled RSS maximum. Dense rebuild sampled PSS maxima
were 1,380.3 and 1,385.9 MiB. These results expose substantial batching/inference
memory at rebuild time; they are not resident-worker memory measurements.
The separate captured `wait4` maximum can include pre-exec launch memory and
must not be subtracted from another process maximum.

## Fresh-process local search

All searches used the final dense configuration, with `--local --no-worker`.
Each of six queries (`cbioportal`, `rust`, `qdrant`, `search`, `embedding`,
`configuration`) ran three times per intent/ranking/filter combination: 108
sequential processes, 18 observations per row. The filter was either absent or
`--provider codex`. Default thread shape, limit and fusion settings were retained.

| Retrieval/ranking | Provider filter | Median ms | Nearest-rank p95 ms |
| --- | --- | ---: | ---: |
| Exact mention / lexical | None | 15.2 | 34.2 |
| Exact mention / lexical | Codex | 15.9 | 22.5 |
| Related discussion / fusion | None | 203.2 | 233.5 |
| Related discussion / fusion | Codex | 216.9 | 263.6 |
| Related discussion / contextual rerank | None | 1,259.4 | 1,551.8 |
| Related discussion / contextual rerank | Codex | 1,582.5 | 1,841.8 |

Observed realized modes were lexical for exact and hybrid for related searches.
Sampled RSS maxima per fusion process were 98.6–104.1 MiB; reranked processes
were 281.2–331.3 MiB. Exact processes commonly finished before the second sample,
so their sampled memory does not reliably characterize peaks.

Wall time includes process launch, model loading, retrieval and JSON output.
Blocking `wait4` measures completion independently of the sampling interval.
Processes were fresh, but filesystem/model pages could be cached; this is not a
cold-disk benchmark. Monitoring overhead is included in wall time and excluded
from child CPU time. Tests ran in fixed order, with no isolation from background
host load. With only 18 observations, nearest-rank p95 is the maximum observed
value; these figures are descriptive, not stable tail-latency estimates.

Reranking is substantially more expensive here than fusion. No relevance labels
were collected, so this comparison cannot establish which policy is better or
justify changing the default. It also does not measure lexical-only search
versus exact search on a dense-capable index.

## Reproduction and remaining gates

Run with an explicitly built binary and installed models:

```bash
python3 tools/benchmark-gold.py \
  --binary /absolute/path/to/mash \
  --models /absolute/path/to/installed-models \
  --output /private/path/outside/the/repository
```

Output must be a new directory outside Git; it is created with mode 0700.
Provider snapshots, per-command stdout/stderr, source hashes and `capture.json`
are private. The measured capture remains in
`/home/james/scratch/mash-artifacts/gold-real-cost-20261003`; only aggregates are
published here. Re-running samples current local files, so source hashes may
change. The private manifest is required to identify this exact corpus.

Follow-ups remain for full-corpus and MacBook measurements, shared Qdrant/HTTP
costs and capabilities overhead, warm/mixed-policy workers, incremental
edits/deletions, model/dimension migrations, backup recovery and held-out
retrieval quality. Those gates stay open under `mash-q3e.6.2`, `.4.2.4`, `.4.5`
and `.5`; this closes only the bounded local measurement child `.6.2.2`.
