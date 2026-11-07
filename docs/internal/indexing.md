# Indexing and models

[Documentation](../README.md) · [Project home](../../README.md)

## Refresh or include older history

```sh
mash index
mash index --all-history
```

The first refreshes using your configured providers and age limit. The second
admits sources regardless of age for this run. Use `--provider codex` to limit
a pass to one provider. For continuous updates, see [watch mode](../synchronization.md).

Indexing reads the provider's original files without changing them. Keep the
local Qdrant service running. After a pass, inspect `mash status` and any errors
reported by the command.

## Enable semantic search

Without installed models, `mash init` configures lexical search. With models
installed, it configures semantic search. A `lexical-only` views build supports
`exact-mention`; semantic `related-discussion` requires `lexical-and-dense`.
Lexical retrieval defaults to `--match-mode flexible` (`match_mode: "flexible"`
in MCP/HTTP): prefixes of at least three characters and conservative typo
corrections use the common Qdrant engine locally and on the server. Use `strict`, `prefix`, or `typo`
to select a narrower mode. Quoted whole queries retain exact spelling, and
numbers are never expanded or corrected. Expanded results include
`lexical_match` with the matched source term. Vocabulary expansion is scoped by
metadata filters and bounded to 100,000 vocabulary terms, 32 query terms and eight alternatives per query
term; a budget error asks for narrower filters or strict matching.
Shared exact-mention queries have a four-second execution budget, including
vocabulary expansion. Use `--match-mode strict` to skip the vocabulary lookup.
The CLI preserves recognized budget errors from the API with this guidance.
Install models explicitly, then build the local index:

```bash
mash models install
mash index
```

If `init` previously configured lexical-only search, change these settings in
your existing TOML before indexing to enable semantic search:

```toml
[views.build]
representations = "lexical-and-dense"

[views.query]
intent = "related-discussion"
```

For an uncustomized setup, `mash init --yes --force` after installing models can
regenerate that configuration and index it. It saves the previous configuration
as `config.toml.previous`; retain custom roots and shared settings when editing.

## Indexing and recovery

Initial indexing is explicit. CLI and MCP search never ingest history, build an
index, or migrate its schema. A missing or incomplete local index returns a typed
error recommending `mash index`, before loading inference models or contacting
the shared search service. An explicitly completed empty index is valid.

Database errors retain SQLite's original reason. Their optional `error.database`
object identifies the operation and includes numeric `sqlite_code` and
`sqlite_extended_code` when SQLite supplies them. `index-busy` remains the
retryable category for both `SQLITE_BUSY` and `SQLITE_LOCKED`; it reports lock
contention, not proof that an indexing writer caused it. For example, a blocked
status read can report `operation: "inspect-status"`, `sqlite_code: 5` and
`sqlite_extended_code: 5` alongside "database is locked". CLI stderr and MCP
structured errors carry the same evidence. No SQL statements or bound values
are added to this diagnostic metadata.

Local mutations also hold a database-scoped `.writer.lock` across silver commits,
model work and views refresh. A competing CLI or MCP writer returns retryable
`index-busy`; readers do not take this lock. Import locks its destination database.
The lock file persists after the operation, but its OS-held lock does not. Do not
delete or replace it while Mash is running. Blocking workers retain ownership if
their caller stops waiting. This coordination does not fence requests already
sent to Qdrant when a process dies.

Canonical operations—`view`, `list-chats`, attachment reads and silver export—
read silver directly. They do not open or repair search views, and remain available when
Qdrant is unavailable or its collections are missing. Import reconstructs silver alone; retrieval
views require an explicit rebuild afterward.

Local indexing separates durable canonical ingestion (silver) from rebuilding
built-in retrieval views, reusing the existing stored history. Lexical indexes
and dense embeddings are built-in views: derived from
silver, certified against it and replayable from it. `mash.sqlite3` holds silver;
Qdrant holds search documents, vectors and certification. Silver uses the same
schema as each node's history on the shared server. Local Qdrant namespaces
include the database's instance identity, canonical path, and node identity, so
copies at another path and imports do not inherit another database's search views.
Readers fail closed when search views are missing, stale, or incompatible.
`mash index --rebuild-local-views` replays retained silver using local Qdrant,
reusing compatible unchanged chat projections. Add `--full` to rebuild every
projection. Silver commits before views mutation; a failed refresh preserves
canonical history and leaves views unready until a successful retry.

For cross-release upgrades, export/import compatibility and backups, see
[upgrades](../upgrades.md). Indexing never upgrades an incompatible database.

`mash index` reports live progress on stderr, including file/vector counts,
elapsed time, and an estimated remaining time for the current phase. Estimates
use recent throughput and adjust as the workload changes; they are approximate,
especially when message lengths vary. Ingestion estimates cover the current
provider; shared-recipe embeddings estimate the current chat.
Model loading, search rebuilding, snapshot preparation, and server
commit show activity with an unknown ETA until work can be measured. Shared
uploads report bytes and an upload ETA; completion is reported only after the
server acknowledges publication (or `shared-pending` if publication fails).
An attempt that skips malformed records ends with `incomplete` and the malformed
count, rather than `complete`; either incomplete state reports that a retry is
required. The CLI exits with status 9 and MCP sets `isError: true` while retaining
the structured partial counts. Correct the source or publication failure and run
`mash index` again; already committed usable data is preserved.
Source coverage and search readiness are reported separately. After a finished
scan, views can be certified over retained history even when some sources were
rejected; valid sources can keep advancing and remain searchable. Ingestion still
reports `incomplete` and exits with status 9 until the source failures are resolved.
Unfinished scans, obsolete interpretation and stale or incomplete search views
remain unready. `index --rebuild-local-views` can rebuild retained history offline
without repairing source coverage or changing the provider files.
Malformed JSONL sources contribute no readable fragments to the index. Their
last complete messages, vectors and checkpoint remain intact until a clean
retry; healthy sources in the same run can still advance.
After a malformed JSONL record, decoding continues to count further failures,
but normalization stops so broken fragment ancestry cannot hide that diagnostic.
For malformed Claude Code and Codex JSONL, `record_diagnostics` in the final
CLI/MCP result contains the first rejected record per source: provider, path,
one-based physical line, byte column, and `syntax`, `data`, or `unexpected-eof`
category. Samples are capped at 32 sources; `omitted_sources` counts additional
affected sources, while `malformed_records` still counts all rejected records.
These diagnostics also appear in progress notifications. Opt-in tracing records
a source fingerprint in place of its path, plus the provider, line, column and
category. Neither includes record content or raw decoder error messages. Final-result diagnostics
remain available when intermediate progress notifications are dropped.
For a recovery checklist that preserves source records and existing index data,
see [recovering incomplete indexing](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/index-recovery.md).
Interactive terminals show an indicatif progress bar or spinner; redirected stderr retains
newline-delimited JSON with `eta_seconds` (`null` when unknown) and `eta_scope`.
The final command response stays JSON on stdout. Watching remains opt-in with
`index --watch` in a separate process.

No other command downloads models. Indexing fingerprints sources and messages,
skips unchanged input, reuses compatible unchanged chat projections, and embeds
changed chats in bounded batches. It excludes tool commands, raw tool output,
reasoning blocks, and unknown roles from lexical retrieval and
embeddings while preserving it for `view`. `--full` rebuilds derived search
state. The default source horizon is 90 days; `--since-days N` overrides it and
`--all-history` removes it for one run.

Messages with empty search projections stay in silver for transcript viewing,
but do not create Qdrant search documents. This includes tool-only activity and
context excluded from dialogue search. If a message's search projection becomes
empty, synchronization removes its previous search document. A full local views
rebuild or shared server replay also removes older empty-projection documents.

Replay reads search projections directly and omits canonical tool payloads and
attachments from its preparation data. Explicit projections avoid decompressing
canonical content; a missing projection still falls back to canonical text.
Small records for hidden messages preserve parent links and inference batch
boundaries. Local fingerprints cover search dependencies, so incremental
refresh can reuse unchanged chat projections after edits only to excluded
canonical context. Explicit full rebuilds still rewrite search documents.

The temporary snapshot uses length-prefixed Protobuf records, with bounded
per-chat frames and no repeated chat IDs. Materialization reads reusable vectors
in groups of up to 256 rows
and prefetches one group while processing the preceding group. Model inference
keeps its original 32-row boundaries, including hidden positions. Writes remain
ordered and must complete before views are certified.

Local and shared destinations use the same Qdrant schema and materialization
code, with separate certification and namespaces. A first publication to both
destinations currently computes their embeddings independently; it does not
forward vectors from a local SQLite cache. Transport benchmarks measure only
publication requests, not this end-to-end indexing cost.

Claude Code and Codex JSONL records are limited to 8 MiB of UTF-8 payload per
line, excluding LF/CRLF. Oversized Codex completed `FileChange` and
`CommandExecution` lifecycle mirrors are validated as JSON and UTF-8 while
streaming, then omitted; their canonical tool records remain indexed as context.
These envelopes have a separate 64 MiB input limit, including line endings.
Malformed envelopes, oversized canonical messages, and unsupported oversized
event types fail indexing. Oversized input fails indexing with a source-specific
error; it is never truncated or marked
complete. A failed configured provider scan retains its previous messages and
checkpoints. These are record limits, not a total chat memory limit.
Codex SQLite history permits individual items up to 64 MiB because file-change
and tool-result items can exceed the JSONL limit. The 65,536-node structural JSON
limit still applies, and large normalized histories spill to disk. Tool bodies
remain canonical context rather than searchable dialogue.

## Models and Apple Silicon

On Apple Silicon, indexing runs the FP32 embedding model through Core ML and
quantizes only the resulting vectors for storage. Batches select a 64-, 128-,
256-, or 512-token session using the model's tokenizer, retaining the 512-token
limit for long messages. Each shape compiles on first use and is cached on disk;
at most four sessions are retained during indexing. Other targets use the
quantized ONNX CPU model. After upgrading an existing Mac installation, rerun
`mash models install` once to install the FP32 asset and compile its Core ML
cache.

Contextual reranking uses CPU by default on every platform. **Experimental GPU
reranking** is opt-in through strict TOML:

```toml
[views.query]
experimental_gpu_reranking = true
```

This enables Core ML on Apple Silicon; other platforms continue to use CPU.
It applies to local and shared searches through both CLI and MCP. No reindex or
MCP restart is needed after changing the setting. Warm MCP searches can benefit,
but first-use compilation and model loading can make fresh CLI searches much
slower than CPU, even with an existing disk cache. Leave it off for ordinary CLI
use.

The experimental path uses four-candidate batches and lazy 64-, 128-, 256-, and
512-token sessions. It uses the already
installed Jina ONNX model; search never downloads assets. First use of a shape
compiles it and later processes reuse its disk cache. A compilation or inference
failure switches that loaded backend to CPU without retrying acceleration on
each batch. Other platforms use CPU reranking.

Search diagnostics include `reranker_engine` on the rerank stage: `core-ml`,
`cpu`, `cpu-fallback`, or `mixed` when separate passes used different engines.
`core-ml` means the complete ONNX graph ran through Core ML; Apple chooses its
CPU, GPU, or Neural Engine placement. It does not certify a particular device.

Core ML cache construction is serialized across processes for each shape.
If an SDK compilation fails, mash tries one fresh cache directory and publishes
it only after successful model construction. Loaded sessions retain their old
cache until they finish; retired caches are cleaned up when no reader uses them.
Indexing reports model-loading progress while waiting or compiling.

## Memory and scratch storage

Local views capture a consistent silver snapshot and spools native messages to a
private temporary file. It materializes one chat at a time with bounded inference
and Qdrant mutation batches. Chat metadata remains in
memory; this is not a process-memory ceiling. There is no SQL embedding queue or
packed-vector cache.

Parser admission limits concurrent estimated retention to a 128 MiB reservation
budget, keeping reservations through queued delivery and SQLite application.
Large sources run alone. Structured indexing progress includes admission counts,
peak reservations and wait time for diagnosing parser backpressure. This is an
estimate rather than a process-memory ceiling; a single large history may need
more memory.

Claude Code and Codex JSONL parsing uses a private, length-bounded file capture.
The parser keeps the original source path and message identities. Before accepting
it, mash verifies that the original file still contains the captured prefix,
using file identity and a content hash. Later appends can remain pending for the
next refresh; checkpoints describe the captured size and modification time.
Edits, truncations or replacement invalidate the old capture and require a fresh
read. Malformed captured records still reject that source; incomplete writes are
not silently dropped or certified. Other providers retain their stable-read
checks. Allow scratch disk space for concurrent input captures as well as
normalized staging: JSONL capture and verification add file I/O, without loading
the whole input into memory.

Normalized message payloads spill to private temporary files above a 16 MiB
inline budget. JSONL parsing, native Copilot CLI parsing and Claude reconciliation
feed this staging incrementally; local storage and shared uploads read it in
bounded batches.
Scratch files use `TMPDIR` and are removed when their owner finishes or fails.
Sensitive scratch SQLite stores use owner-only directories and files on Unix.
Checksums detect corrupt staging before publication. Metadata still grows with
message count, and some native adapters buffer their input before normalization.
See the [large-history staging measurement](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-01-normalized-message-staging.md)
for the measured memory and disk-I/O tradeoff.

Claude native ancestry spills to private scratch SQLite above a 2 MiB inline
accounting budget; parent-chain cycle detection retains only two UUIDs. Exact
parent states and compaction replay behavior are preserved, and scratch failures
abort ingestion of that source. Compaction duplicate membership and saved original
parents share a separate store with the same 2 MiB inline accounting budget;
restoration streams one entry at a time, and each finished or interrupted replay
block releases its scratch store. These budgets are accounting thresholds, not
process RSS limits. Ordered dialogue UUIDs and their first native positions use
a third private store with a 2 MiB inline accounting budget. Missing UUID slots,
duplicate first positions and forward ancestry references remain exact.
Reconciliation lineage, spool offsets, model workspaces and other adapters still
have separate memory costs.
The [ancestry staging measurement](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-02-claude-ancestry-staging.md)
records the whole-index memory reduction and indexing-time tradeoff.
The [replay staging diagnostic](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-claude-replay-staging.md)
separately compares large compaction metadata with its previous in-memory rules.
The [dialogue identity diagnostic](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-claude-dialogue-identities.md)
records its separate memory reduction and additional scratch-I/O cost.

## Provider history and identity

Without a configuration file, mash indexes these built-in roots when present:

- Claude Code: `~/.claude/projects`, `~/.config/claude/projects`
- Codex: `~/.codex/sessions`, `~/.local/share/codex/sessions`, and each directory's sibling `thread_history_1.sqlite`

Claude subagent transcripts share their parent's native session ID. mash uses
the session and native `agentId` together to index each subagent as an independent
chat, so it cannot replace the parent's history. Search never indexes history implicitly.

Claude message UUIDs are scoped to their native session and agent, independent
of the transcript's physical path. SQLite retains each physical source's
contribution and derives one logical chat from their union. Event-only workflow
journals do not create chats or require
a single agent identity. Agent consistency is enforced when the source contains
retained dialogue, including identity supplied by metadata records. Matching copies
are deduplicated; divergent branches retain their own causal context. Rewinding,
emptying, or deleting one copy removes only that copy's contribution. Conflicting
payloads or ancestry for the same native message UUID fail transactionally.
Repeated metadata events with conflicting parents are ignored when no retained
dialogue depends on them; ambiguous events in a message's parent chain still fail.
Compaction may append reduced copies of earlier dialogue with rewritten parents.
Mash retains the original archived ancestry only for an uninterrupted block of
existing UUIDs followed by a native compaction boundary whose logical parent and
preserved tail identify the preceding event outside that block. Replayed message
ancestry can also survive a new native `stop_hook_summary` directly linked to
the last duplicate, when that summary is confirmed by the next boundary. Another
new event or duplicate block discards the pending replay evidence. Replayed message
content must still match; unrelated reparenting and incomplete replay evidence
fail transactionally. The native compaction boundary remains a context root.
Native forks may retain the original session ID on their copied history. When
the new session's dialogue explicitly links into that history, the whole copied
transcript belongs to the fork's native session; the original remains a separate
chat. Nested forks follow the same rule. Unlinked transitions and interleaved
session IDs fail rather than silently combining unrelated histories. Records
without a UUID retain path-based fallback identity.

Shared publication uses this same reconciliation. Changing old path-based message IDs can require
re-embedding Claude messages; unrelated providers' vectors remain intact.
Source snapshots increase SQLite storage, especially with many physical copies.

## Tracing and diagnostics

For pipeline diagnostics, set `MASH_TRACE=1`. mash writes JSON trace events to
stderr, including process/request IDs, request elapsed milliseconds, nested stage spans, span busy/idle timing,
safe error kinds, and HTTP status codes. CLI JSON and MCP messages remain on
stdout. Only mash's own trace targets are enabled; dependency logs are excluded.
Trace fields omit query/history text, paths, filters, credentials, and vectors.
Existing indexing progress events can also appear on stderr.

Index traces separate discovery and checkpoint filtering, parser admission waits,
parsing retries, queue sends, application, embedding planning and wave reads,
CPU/Core ML inference batches, vector writes, and commits. Admission events
include the estimated reservation, budget, elapsed wait, and whether capacity
forced a wait. Inference events report batch counts and Core ML sequence shape;
they never include the texts or vectors. Request context follows parser and CPU
inference threads through the same propagation helper.

For example, `MASH_TRACE=1 mash search cbioportal --local 2>trace.jsonl`
exposes model loading, embedding, lexical retrieval, vector loading/scoring and
reranking. Use `MASH_TRACE=1 mash mcp` for repeated requests or
`MASH_TRACE=1 mash-server --server-config server.toml` on the shared server.
Spans describe inclusive lifetimes, not CPU time; nested and concurrent timings
must not be added together. Request IDs are
local to a process, paired with process IDs. With tracing enabled, each shared
HTTP call sends an opaque `x-mash-trace-id`; its `shared_http_request` client
span and the server's `http_request` span record the same `trace_id`. Enable
tracing on both processes and match that field to correlate status and timing.
The server replaces missing, malformed or duplicate IDs with a fresh local ID;
client tracing disabled sends no correlation header.

For async task and resource inspection, build with `--features console` and
`RUSTFLAGS='--cfg tokio_unstable'`, then run that binary with `MASH_CONSOLE=1`.
Connect `tokio-console http://127.0.0.1:6669` to its loopback telemetry listener.
This is independent of `MASH_TRACE`; ordinary builds omit console instrumentation.
See the [performance profiling skill](../../.agents/skills/mash-performance-profiling/SKILL.md)
for isolated builds, remote access and measurement limitations.

Successful shared response bodies are decoded into typed models on blocking
workers so large JSON responses do not occupy an async runtime worker. The
`shared_http_decode` child span records byte count, `mode` and success/error, with the
same request ancestry and no response contents. HTTP request timing includes
admission, network I/O, blocking-worker queueing and decoding. In `buffered` mode
the child measures parsing; in `streamed` mode it also includes waiting for body
frames. Streamed byte counts include reader read-ahead. The request span records `queue_ms`, the fixed `endpoint` name and
`admission` class. Each process admits up to eight interactive requests and one
bulk state/publication request. They use separate capacity, acquired before
sending HTTP. The 5-second search, 2-second metadata/context or 30-minute bulk deadline
includes queueing and waiting for decoding. A canceled decoder retains its
capacity until the blocking work finishes; cancellation does not interrupt
already-running JSON parsing. This bounds concurrent response work, not response
bytes or the number of pending callers. Node state streams immediately; other
responses use the fast buffered parser up to a 1 MiB preview, then stream through
a 64 KiB reader. This removes whole-wire retention for large responses without
truncating histories. Canceling a streamed request wakes blocked body I/O; it
does not preempt CPU parsing. Owned typed results and upstream frame buffers still
consume memory. Large-history paging and owned-data budgets
remain tracked in `mash-c0d.21.4`.
See the [response decoding checks](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-shared-response-decoding.md)
for concurrency and relevance verification.
The [response admission checks](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-shared-response-admission.md)
cover queue expiry, cancellation, separate capacity and remaining memory limits.
The [streaming control](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-shared-response-streaming.md)
documents the large-state memory/parsing tradeoff and body cancellation checks.

## Additional provider formats

OpenCode is opt-in through `[providers.opencode]`, with `roots` pointing to
`opencode.db` (normally `~/.local/share/opencode/opencode.db`) or its containing
directory. It reads sessions and project metadata from native SQLite snapshots,
including WAL changes. Each session becomes one chat. Sessions with
`session_message` records use that history; other sessions use the native
`message`/`part` tables. User and assistant text is searchable; reasoning, tools,
files, shell output, compaction and synthetic/ignored text remain context only.
Operational events and unknown part types are omitted. Session activity controls
the history horizon. OpenCode JSON records are checked for the 8 MiB byte limit
and a 65,536-node structural limit before typed decoding. OpenCode sessions have
a 64 MiB cumulative text-read budget and a 65,536-record limit; normalized
messages and individual legacy-message assembly have a 32 MiB retention budget.
Session metadata is capped at 8 MiB before fetching it. Exceeding these limits
fails indexing and preserves the previously indexed snapshot and checkpoint.
These parser budgets do not measure process RSS across concurrent workers.
The adapter follows the upstream
[database schema](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/sql.ts)
and [message schema](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session-message.ts).

GitHub Copilot CLI is opt-in through `[providers.copilot]`, with
`roots = ["/absolute/path/to/.copilot/session-state"]` or explicit `.jsonl`
event files. CLI directory discovery accepts native `events.jsonl` logs.
The adapter reads the v1 event schema: displayed human requests and complete
main-agent answers are searchable. Streaming deltas are omitted; augmented
model prompts and binary attachments are not indexed. Tools, reasoning,
injected prompts, autopilot continuations and subagent messages remain context
only. Each agent has a separate context chain, so interleaved subagents cannot
become neighbours of a main-agent search match. Clearing model context starts
a new chain while preserving earlier history; native file rewinds remove the
discarded messages on the next index run. Session IDs namespace message IDs,
including when a session is forked. The first settled logged workspace supplies
project and repository metadata; repository identifiers are kept as logged.
Missing headers, unsupported versions and incomplete records fail explicitly,
preserving the previous indexed snapshot until a complete refresh succeeds.
The native format follows the official [Copilot event schema](https://github.com/github/copilot-sdk/blob/main/nodejs/src/generated/session-events.ts)
and [VS Code history reader](https://github.com/microsoft/vscode-copilot-chat/blob/main/src/extension/chatSessions/copilotcli/common/copilotCLITools.ts).

Regular **Copilot Chat in VS Code** uses a separate adapter under the same
`[providers.copilot]` setting. Add VS Code's `User/workspaceStorage` and
`User/globalStorage` directories, or explicit `state.vscdb` files, alongside
the CLI root. On macOS these are under
`~/Library/Application Support/Code/User`; on Linux usually `~/.config/Code/User`;
on Windows `%APPDATA%/Code/User`. Insiders and custom user-data directories can
be configured the same way. Both formats can be indexed together:

```toml
[providers.copilot]
roots = [
  "/absolute/path/to/.copilot/session-state",
  "/absolute/path/to/Code/User/workspaceStorage",
  "/absolute/path/to/Code/User/globalStorage",
]
```

The read-only VS Code catalog selects local sessions, including empty-window
chats. Orphaned files and external CLI/cloud session entries are excluded.
Native JSON snapshots (including older plain-string responses) and JSONL
mutation logs are supported; logs take
precedence over retained snapshots. Edits, response updates and array rewinds
retain stable identities and remove discarded history on refresh. A partial
or invalid log preserves the previous indexed snapshot for retry. Draft input
is excluded. Human requests and answer markdown are searchable; hidden requests,
reasoning, tools and explicitly identified third-party participants remain
context only. Workspace and logged repository metadata support normal filters.
CLI and Chat session/message IDs have distinct namespaces. Both use the existing
`copilot` provider filter and provider settings. Their upstream schemas are
the [VS Code session store](https://github.com/microsoft/vscode/blob/f5f73df304d9c5eeab64d4d07eed57e007742a92/src/vs/workbench/contrib/chat/common/model/chatSessionStore.ts)
and [typed mutation schema](https://github.com/microsoft/vscode/blob/f5f73df304d9c5eeab64d4d07eed57e007742a92/src/vs/workbench/contrib/chat/common/model/chatSessionOperationLog.ts).

Cursor Agent is opt-in through `[providers.cursor]`. Configure its native `chats`
directory or an explicit `store.db`. Its config root is `CURSOR_CONFIG_DIR`, then
`$XDG_CONFIG_HOME/cursor`, then `~/.cursor`; chat databases are below
`chats/<workspace-hash>/<session>/store.db`. The reader takes read-only WAL-aware
snapshots and follows the latest root's reachable archives and current messages.
Repeated requests retain distinct identities through archive moves and rewinds.
Tools, reasoning, model summaries, simulated prompts and subagent histories stay
context-only; current user messages require matching native turn evidence.
Native workspace URIs and tracked repositories supply project/repository filters.

Each blob is limited to 8 MiB, each protobuf to 65,536 fields, cumulative reads
to 64 MiB and retained normalized history to 32 MiB. Missing, corrupt or oversized
reachable history fails indexing and preserves the previous revision. These are
parser limits rather than process RSS limits. Cursor IDE is also supported through
explicit `state.vscdb` files or directories containing them. It reads the native
application KV schema and v18 composer roots, follows saved bubble order, and
keeps native bubble IDs stable through edits and rewinds. Drafts, summaries,
orphaned bubbles, simulated/agent-sent prompts and operational rows do not enter
search prose. Older or unknown root versions fail explicitly. Each JSON record
is limited to 8 MiB and 65,536 nodes; each chat has a 64 MiB read budget
and 32 MiB retained-history budget. IDE and Agent identities remain distinct.
Its [native storage contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/cursor-ide-ingestion-contract.md) records the
fixture provenance and remaining captured-session validation gates.
The [Cursor ingestion contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/cursor-ingestion-contract.md)
records first-party fixture provenance and remaining real-session validation gates.

Antigravity IDE 2.5.5 is opt-in through `[providers.antigravity]`. Configure
`~/.gemini/antigravity-ide/conversations` or explicit `<cascade-id>.db` files.
Mash opens a read-only SQLite snapshot, including committed WAL changes; no
editor or language server is required. Repeated prompts have distinct native
step IDs, and surviving IDs remain stable after a history suffix is removed.
Displayed user prose and assistant responses enter search. Thinking, attached
text-block bodies, system steps, subagent context and copied fork history do not.
Supported reasoning and text-block attachments remain available in canonical
message views. Other attachment types, tool-step bodies and subtrajectories are
not currently reconstructed.

Each native BLOB is limited to 8 MiB, each chat to 64 MiB of reads,
65,536 steps/decoded fields and 32 MiB of retained normalized history. Unknown
step formats, SQL/protobuf disagreements, malformed timestamps and oversized
input fail explicitly and preserve the previous index revision. Native workspace
and Git-root URIs supply project/repository filters when unambiguous. The
[Antigravity contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/antigravity-ingestion-contract.md) records native
writer/fork evidence and remaining real UI, successful rewind, CLI/Hub and
cross-version validation gates. These limits do not establish aggregate RSS.

For tree histories, `view --context N` includes up to N actual ancestors and
up to N messages along the first saved continuation from the match. It never
adds sibling replies because their file ordinals happen to be nearby. Contextual
reranking follows the same rule for local and shared search. Linear histories
keep their existing ordinal neighbourhoods. Cross-release schema changes use
silver export/import and a search-views rebuild. Update the shared service before
publishing Copilot, Cursor or Antigravity histories from upgraded nodes.

Codex paginated history is read from an explicitly configured
`thread_history_1.sqlite`, with its sibling `state_5.sqlite` as a required metadata
dependency. Both databases are opened read-only and changes in either WAL invalidate
the snapshot. Built-in discovery includes these stores; explicit provider roots
must include them separately from `sessions`. Installation diagnostics warn when
a store exists beside a configured sessions directory but is not covered.
Thread IDs are preserved. For a paginated thread with stored items, the SQLite
source replaces only the JSONL mirror named by Codex metadata. Other JSONL sources
remain readable. Dialogue is searchable; reasoning, hooks and tool context remain
canonical context. Realtime transcript segments share the recorded rollout order;
promotion markers reference existing items without duplicating their content.
Unsupported item types reject the source
instead of silently replacing retained history with a partial transcript.
This reads locally available history; it does not fetch cloud-only pages or recover
records that Codex has removed. Keep exports when migrating old indexes.

Codex JSONL histories include raw response records and completed `UserMessage` /
`AgentMessage` turn items. Nearby raw/completed counterparts are paired once;
unmatched dialogue and separate repeated requests are retained. Started items,
streaming deltas and legacy event mirrors are omitted. See the
[native Codex contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/codex-ingestion-contract.md) for migration fixtures
and reconciliation limits. Search does not migrate or ingest. Refresh compatible indexes explicitly with
`mash index`; see [upgrades](../upgrades.md) for cross-release changes.
