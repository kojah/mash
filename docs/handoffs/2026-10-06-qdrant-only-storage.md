# Qdrant-only storage handoff — 2026-10-06

Work paused at the user's request to conserve credits. **The cutover is not complete.**
Do not deploy the intermediate implementation or mark the goal complete.

## Objective and acceptance

Qdrant must be the only Mash persistence backend, locally and remotely: silver,
gold, metadata, checkpoints, tombstones and publication state. No Mash-owned
SQLite database, staging database, cache or fallback. Read-only ingestion of
provider-owned SQLite histories remains supported. Local operation must work
offline against local Qdrant; shared operation goes through authenticated Axum
HTTP, with one index per node group. CLI/MCP call the same typed core.

Local/shared search must be reasonably fast, with no glaring easy performance
fixes outstanding. HNSW is the default dense policy; verify actual native planner
execution and quality, not just `exact=false`. Missing initial indexing must fail
without ingesting or initializing storage. Preserve progress, edits/deletions,
native Claude ancestry, attachments, explicit export/import and restart recovery.

The detailed acceptance contract is `docs/qdrant-storage-cutover.md`. Its full
scope still applies. Do not reduce acceptance to the passing exchange/search tests.

## Authoritative locations and ownership

- Main repository: `/home/james/scratch/mash`, branch `main`, GitHub upstream
  `https://github.com/kojah/mash.git`.
- **Implementation WIP:** detached worktree
  `/home/james/scratch/mash-worktrees/search-performance`, based on `70c5727`.
  Continue there. Its changes are uncommitted and not deployed.
- Main contains unrelated pre-existing RAM-profile and clustering-research edits.
  Preserve them. They include README/CLI, Qdrant memory/server modules, paper
  changes and six untracked research files. Never stage, revert or overwrite them.
- Private artifacts/logs: `/home/james/scratch/mash-artifacts/search-performance-20261006`.
- Recovery copies of the owned WIP are in that directory:
  `qdrant-cutover-tracked-wip.patch` and `qdrant-cutover-wip-files.tar.gz`.
  The tar includes untracked implementation files; the patch does not.
  These contain code, not the private history corpus.
- Beads: `mash-ua2` is the storage cutover; `mash-c06` tracks performance.

Run `git status --short` in both repositories before editing. Main may have
advanced with this handoff commit; the detached implementation base remains unchanged.

## Committed foundation

Already pushed before the current WIP:

- `81ee0e3`: native filtered-HNSW thresholds and explicit query activation.
- `0dffc92`: fault deadline begins after connection.
- `5f22988`: storage-independent history/retrieval records and acceptance contract.
- `a83982c`: typed versioned canonical JSON Lines exchange, replacing SQL exports.
- `70c5727`: immutable canonical Qdrant chunks, verified reads and conditional
  head publication, including UTF-8-safe chunking and appropriate schema-write deadlines.

The running shared deployment was last verified at `0dffc92`. The newer local
cutover code has not been deployed. Preserve original source databases and exports.

## Current implementation WIP

### Configuration and exchange

`app/config/local.rs` adds strict `[local]` TOML: endpoint/key environment-variable
references, namespace, snapshot limit and retained-file directory. Default URL is
`http://127.0.0.1:6333` through `MASH_LOCAL_QDRANT_URL`.

Core CLI/MCP export/import now use Qdrant through `app/export/qdrant.rs`.
Import names a fresh namespace rather than a database file. Validation occurs
before setup; existing destinations are refused. Canonical record bytes and
retained attachment checksums are preserved. SQL conversion remains test-only
for legacy migration fixtures, pending final removal.

### Gold and search

`app/shared/local.rs` rebuilds gold from a captured canonical Qdrant archive using
the existing typed shared gold schema and concrete embedding implementations.
Canonical heads now carry an optional gold reference. Silver changes invalidate
it; conditional publication rejects a build against an outdated source.
The index response reports the actual stable Qdrant model-space identity.

`app/shared/local/read.rs` holds canonical/gold identity across a request, opens
existing collections without setup, checks model compatibility before inference
and checks publication identity through final ranking. Local context batches use
one catalogue snapshot and at most four concurrent readers.

`app/shared/retrieval/native.rs` handles local, combined and shared semantic
retrieval: one query embedding, concurrent local Qdrant/shared HTTP candidates,
existing per-source RRF, bounded thread/message reranking and contextual evidence.
Local exact search and concurrent combined exact search also use Qdrant.
The **production core search path no longer opens SQLite**. Legacy SQL search,
vector and filter helpers are test-only until their fixtures are replaced.

Local calls go directly to Qdrant's native HTTP API, not the Mash Axum service.
Shared calls use Axum. Its bearer tokens bind callers to node/group identities;
local CLI/MCP needs no shared token or login.

A combined-HTTP test caught a cutover bug: compact shared hits were fused while
their bodies were still empty. Typed projections are now restored **before**
fusion; a default unit test guards both message/thread modes.

### Latest staging work

`app/silver/messages/ledger.rs` extends the existing typed private message spool
to share one closed file across many threads, using bounded frame ranges and
per-thread ordinal ordering. Message bodies are not retained in a corpus-sized
RAM map. Path ownership is shared until the last reader drops it. Checksums,
bounds, duplicate-ordinal rejection and file-length checks remain enforced.
`reader.rs` avoids seeking/discarding buffered data for contiguous frames.

`app/shared/local/capture.rs` now uses that ledger for gold input. It parses the
canonical archive once, handles enrichment before its thread declaration, groups
interleaved messages and preserves native parent edges. CPU preparation and
buffered gold writes run on blocking workers; embedding progress is emitted.

**This is an input primitive for the ingestion port, not completed provider
ingestion.** The provider/checkpoint reconciliation driver still uses SQLite.
Do not describe staging improvements as finishing that port.

## Evidence and pending checks

Passing logs in the private artifact directory:

- `native-dense-manifest-final-2.log`: real-model native gold rebuild, filtered
  message/thread searches, local context, offline shared fallback and successful
  typed HTTP peer retrieval; model-space receipt checked; 2.575-second small test.
- `native-dense-unit-final.log`: 24 config/redaction/compact-projection tests passed.
- `native-dense-doc-final.log`: doctests passed.
- `native-dense-manifest-clippy-2.log`: strict all-target Clippy passed.
- `native-capture-core.log`: latest capture path passed MCP exchange/exact and
  real-model semantic/HTTP regressions; two tests passed in 3.278 seconds.
- `native-capture-units-final.log`: ten staging/capture/redaction tests passed,
  including arbitrary record ordering, parent edges, lifetime and corruption cases.
- `native-capture-clippy-final.log` and `native-capture-seek-clippy.log`: strict
  all-target Clippy passed, including the latest contiguous-read optimization.

The full CLI suite **does not pass during this transition**. Ordinary provider
indexing still writes SQLite while search/exchange/rebuild now expect Qdrant.
Other metadata/publication paths also remain SQL-backed. Fix these workflows;
do not skip tests, suppress behavior or claim full-suite completion.

Small synthetic search timings are not real-corpus performance acceptance.
Earlier deployed SQLite local timings likewise do not prove the new local path.

## Real-corpus capture benchmark

The existing private canonical export contains 817 threads, 345,451 messages,
1,026,902,224 record bytes and 65 retained files (12,887,017 bytes).
Original SQLite and archive data remain untouched.

`native-capture-corpus.log` hit the actual 120-second nextest termination limit.
This was a timed-out benchmark, not a proved correctness failure. The manual
ignored benchmark now has a targeted 600-second budget and prints separate
capture/total time and Linux peak RSS; it asserts counts and peak RSS below 512 MiB.
It includes body redaction while draining captured batches and does not run models.

The longer run was `native-capture-corpus-2.log`, execution session `33267`,
nextest PID `2263224`. **SIGINT was sent when the user requested stopping.**
It is terminal: the log records SIGINT at 55.982 seconds and the command exited
100; process inspection found no remaining nextest/Clippy processes. This is an
interrupted run, not a correctness result. Do not restart it automatically.
The session handle may not survive a new conversation. Temporary data for that
run is isolated in `native-capture-benchmark-scratch` under the artifact directory.
Interrupted tests can leave private temporary files; remove only proven owned
benchmark files. Never delete original archives/databases or unrelated scratch.

No completed large-corpus capture timing/peak-RSS result is established yet.
On resume, finish this profiling only as needed. Potential duplication in content
and identical dialogue-text redaction has not yet been measured or optimized.

## Remaining implementation — next priority

1. Port **ordinary provider ingestion** and checkpoint/source reconciliation to
   Qdrant. Reuse discovery, stable-prefix capture, bounded parser admission,
   normalized messages, enrichment and pure Claude reconciliation. Preserve
   authoritative-root rollback, rejected-source retention, deletions, tombstones,
   interpretation versions, attachment policy and native source contributions.
   Relevant code: `app/silver.rs`, `app/storage/indexing.rs`,
   `app/storage/claude_sources.rs`, `app/storage/silver_state.rs` and attachments.
   Do not introduce a SQLite staging database or narrow this to one provider.
2. Move shared publication/rebuild/attachment upload off the SQL local source.
   Preserve per-index provider allowlists and complete source certification.
3. Port status, list-threads, canonical view, forget and attachment access.
   These must work without models or ready gold. Reading the entire gigabyte
   archive for every metadata request is not an acceptable final design.
4. Implement true source/thread deltas, unchanged-vector reuse and bounded
   reclamation. Current canonical full-archive transfers/retained old chunks are
   not a complete efficient watch pipeline.
5. Remove owned SQLite schema, queries, sqlc/build hooks and obsolete tests after
   complete native workflows exist. Keep only read-only provider adapters.
6. Run the complete required suite and native restart/recovery/CLI/MCP tests,
   real-data quality/filter comparisons, local/combined cold/warm measurements
   and actual HNSW telemetry. Then commit/push the owned implementation, merge
   it into main preserving unrelated dirty changes, and deploy reachable nodes.

## Commands and operational constraints

Use Cranelift for iteration and persistent paths:

```bash
cd /home/james/scratch/mash-worktrees/search-performance
env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --locked --lib -E 'test(...)'
```

Final checks use `CARGO_INCREMENTAL=0`, full `cargo test-clif --locked --profile ci`,
`cargo fmt --all --check`, `cargo test --locked --doc` and all-target Clippy with
`RUSTFLAGS=-Znext-solver=no` to avoid the known nightly solver ICE.

Owned isolated service: `mash-qdrant-local-core-20261006`, Qdrant 1.19.1,
label `mash.owner=local-core-cutover-20261006`, last native port `45482`.
It contains synthetic fixtures and was left running, idle. Rediscover its port
and verify ownership before using it. **Do not stop the live `mash-qdrant` or
shared API service.** Other benchmark containers/artifacts may remain; inspect
exact labels and current processes before changing them.

Do not use Codebase Memory for Mash: user instructions restrict it to gohawk.
Do not spawn agents without explicit authorization. Keep private data/logs out of
Git and avoid raw history or secrets in outputs. The root RAM patch uses
`MemoryPolicy`/default `Durable`, while this worktree uses `MemoryTier`/`Cached`;
existing `Default::default()` initializer calls accommodate that difference,
but merging requires validation against the root patch.

The user's Git policy requires a focused implementation commit and push after
relevant validation. Current unfinished runtime WIP has intentionally not been
committed/deployed. This handoff is a separate documentation commit. Beads uses
Dolt: commit/push its notes without committing its runtime files or credentials.
