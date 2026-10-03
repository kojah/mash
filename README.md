# mash

Agent work is tracked with [Beads](https://github.com/steveyegge/beads). With `bd`
and Dolt installed, run `bd bootstrap` after cloning, then `bd prime` and `bd ready`.
Issue history lives on a separate Dolt Git remote in this GitHub repository;
`bd dolt commit` and `bd dolt push` persist tracking updates. The entire `.beads/` directory, including configuration, stays local and is ignored
by Git. `bd bootstrap` discovers the Dolt history from the Git origin.

mash (Multi-Agent Search History) searches Claude Code, Codex, and Aider histories
through a JSON CLI and an MCP server built with the official Rust SDK. Nodes can
keep a local index and optionally publish to a shared service over HTTP, with independent provider allowlists.
SQLite FTS5, local FastEmbed embeddings, reciprocal-rank fusion, and bounded
cross-encoder reranking provide
hybrid retrieval. Nodes optionally publish to a joint shared index, one per
node group, so shared history remains searchable while source machines are offline.
The service uses Qdrant as its sole shared storage backend. Local SQLite is required for offline search.

## Commands

SQLite query models are generated with sqlc 1.31.1 / sqlc-gen-rust 0.1.12.
Generated output is checked in; normal Cargo builds require no generator.
To regenerate as part of a build, install these versions and run
`make build CODEGEN=1` (add `CARGO_ARGS='--release --locked'` for release).
`make generate-queries` regenerates without compiling; `make check-generated`
checks for stale output. Generation reads the same SQLite schema SQL used at runtime.
CI checks regeneration against committed output. Dynamic predicates and FTS ranking
reuse the generated projection and decoder; migrations and writes remain explicit.

```text
mash index [--full|--watch [--debounce-ms N]] [--provider claude-code|codex|aider|hermes|opencode|openclaw|pi|copilot|gemini-cli|cline]... [--since-days N|--all-history]
mash index --rebuild-local-gold [--full]
mash search <query> [--unit threads|messages] [--local | --shared] [--no-worker] [--limit N] [--provider PROVIDER] [--days N]
mash list-threads [--local | --shared] [--provider PROVIDER] [--title-contains TEXT] [--after UNIX_SECONDS] [--before UNIX_SECONDS] [--sort updated-at|created-at|title] [--direction asc|desc] [--limit N] [--offset N]
mash view <message-id> [--shared NODE_ID] [--context N]
mash status [--shared]
mash forget <conversation-id>
mash models install
mash mcp [--watch [--debounce-ms N]]
mash serve --server-config PATH
```

One-shot data commands emit one JSON value; watch mode emits newline-delimited
results, and `mash mcp` serves the stdio protocol. `mash serve` runs the shared
index HTTP API; it reports its listening address as JSON on stderr and keeps
stdout empty. `mash --help` and subcommand `--help` return `{"help":"..."}`;
`mash --version` returns `{"version":"..."}`. These informational requests
exit successfully before loading configuration, models or indexes. Bare `mash`
returns a JSON usage error containing help with exit code 2. Global
options are `--config PATH`, `--db PATH`, and
`--models-dir PATH`.

On Linux and macOS, CLI searches automatically share an on-demand local worker
for the same configuration, database, model directory, and credential environment.
The first search loads models; subsequent commands reuse the models and shared
HTTP connections, even for different queries. The worker exits after 60 seconds
without requests and releases its memory. Concurrent searches queue through the
same worker. It accepts only searches through a private per-user Unix socket;
no system service is installed.

Use `mash search "query" --no-worker` to run entirely within that CLI process.
If the worker cannot be reached, search runs directly. Configurations and index
data are read for every request; results are not cached. MCP sessions continue
to reuse models within their own process and do not launch CLI workers.

## Models and indexing

Search is semantic by default and has no production lexical-only mode. Install
models explicitly, then build the local index:

```bash
mash models install
mash index
```

Initial indexing is explicit. CLI and MCP search never ingest history, build an
index, or migrate its schema. A missing or incomplete local index returns a typed
error recommending `mash index`, before loading inference models or contacting
the shared search service. An explicitly completed empty index is valid.

Local indexing separates durable canonical ingestion (silver) from rebuilding
retrieval views (gold), reusing the existing stored history. See the
[history pipeline](docs/history-pipeline.md) for layer ownership, storage schemas
and recovery behavior.

`mash index` reports live progress on stderr, including file/vector counts,
elapsed time, and an estimated remaining time for the current phase. Estimates
use recent throughput and adjust as the workload changes; they are approximate,
especially when message lengths vary. Ingestion estimates cover the current
provider; independently routed shared embeddings estimate the current conversation.
Model loading, search rebuilding, snapshot preparation, and server
commit show activity with an unknown ETA until work can be measured. Shared
uploads report bytes and an upload ETA; completion is reported only after the
server acknowledges publication (or `shared-pending` if publication fails).
An attempt that skips malformed records ends with `incomplete` and the malformed
count, rather than `complete`; either incomplete state reports that a retry is
required. The CLI exits with status 9 and MCP sets `isError: true` while retaining
the structured partial counts. Correct the source or publication failure and run
`mash index` again; already committed usable data is preserved.
A partial initial run remains unready for search until a clean indexing attempt
finishes. Malformed input alone does not invalidate an unchanged, previously
completed local index.
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
see [recovering incomplete indexing](docs/index-recovery.md).
Interactive terminals show an indicatif progress bar or spinner; redirected stderr retains
newline-delimited JSON with `eta_seconds` (`null` when unknown) and `eta_scope`.
The final command response stays JSON on stdout. Watching remains opt-in with
`index --watch` or `mcp --watch`.

No other command downloads models. Indexing fingerprints sources and messages,
skips unchanged input, updates FTS incrementally for small changes, embeds only
new or changed searchable messages, and excludes tool commands, raw tool output,
reasoning blocks, and unknown roles from FTS and
embeddings while preserving it for `view`. `--full` rebuilds derived search
state. The default source horizon is 90 days; `--since-days N` overrides it and
`--all-history` removes it for one run.

Claude Code and Codex JSONL records are limited to 8 MiB of UTF-8 payload per
line, excluding LF/CRLF. Oversized Codex completed `FileChange` and
`CommandExecution` lifecycle mirrors are validated as JSON and UTF-8 while
streaming, then omitted; their canonical tool records remain indexed as context.
These envelopes have a separate 64 MiB input limit, including line endings.
Malformed envelopes, oversized canonical messages, and unsupported oversized
event types fail indexing. Aider uses the same line limit and an 8 MiB limit per
complete turn, including code fences and embedded newlines. Oversized input
fails indexing with a source-specific error; it is never truncated or marked
complete. A failed configured provider scan retains its previous messages and
checkpoints. These are record limits, not a total conversation memory limit.

Upgrading an existing index to schema 18 requires an explicit `mash index`.
The migration retains canonical history and existing vectors, preserves recorded
project/repository metadata, and invalidates source checkpoints and search
readiness so projections are refreshed before searching.

On Apple Silicon, indexing runs the FP32 embedding model through Core ML and
quantizes only the resulting vectors for storage. Batches select a 64-, 128-,
256-, or 512-token session using the model's tokenizer, retaining the 512-token
limit for long messages. Each shape compiles on first use and is cached on disk;
at most four sessions are retained during indexing. Other targets use the
quantized ONNX CPU model. After upgrading an existing Mac installation, rerun
`mash models install` once to install the FP32 asset and compile its Core ML
cache.

Core ML cache construction is serialized across processes for each shape.
If an SDK compilation fails, mash tries one fresh cache directory and publishes
it only after successful model construction. Loaded sessions retain their old
cache until they finish; retired caches are cleaned up when no reader uses them.
Indexing reports model-loading progress while waiting or compiling.

Embedding planning uses a temporary SQLite file to deduplicate and sort pending
texts, loading one inference wave at a time. Duplicate message IDs are written
in bounded pages. Large rebuilds therefore avoid retaining all pending texts in
memory; the model workspaces and parsed conversations have separate memory costs.

Parser admission limits concurrent estimated retention to a 128 MiB reservation
budget, keeping reservations through queued delivery and SQLite application.
Large sources run alone. Structured indexing progress includes admission counts,
peak reservations and wait time for diagnosing parser backpressure. This is an
estimate rather than a process-memory ceiling; a single large history may need
more memory.

Normalized message payloads spill to private temporary files above a 16 MiB
inline budget. JSONL parsing, native Copilot CLI parsing and Claude reconciliation
feed this staging incrementally; local storage and shared uploads read it in
bounded batches.
Scratch files use `TMPDIR` and are removed when their owner finishes or fails.
Sensitive scratch SQLite stores use owner-only directories and files on Unix.
Checksums detect corrupt staging before publication. Metadata still grows with
message count, and some native adapters buffer their input before normalization.
See the [large-history staging measurement](benchmarks/2026-10-01-normalized-message-staging.md)
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
The [ancestry staging measurement](benchmarks/2026-10-02-claude-ancestry-staging.md)
records the whole-index memory reduction and indexing-time tradeoff.
The [replay staging diagnostic](benchmarks/2026-10-03-claude-replay-staging.md)
separately compares large compaction metadata with its previous in-memory rules.
The [dialogue identity diagnostic](benchmarks/2026-10-03-claude-dialogue-identities.md)
records its separate memory reduction and additional scratch-I/O cost.

Without a configuration file, mash indexes these built-in roots when present:

- Claude Code: `~/.claude/projects`, `~/.config/claude/projects`
- Codex: `~/.codex/sessions`, `~/.local/share/codex/sessions`

Claude subagent transcripts share their parent's native session ID. mash uses
the session and native `agentId` together to index each subagent as an independent
thread, so it cannot replace the parent's history. An explicit `mash index`
upgrades older indexes and reprocesses Claude source checkpoints to repair this
identity collision; search never performs the upgrade or indexing itself.
The schema 32/33-to-34 upgrade preserves existing vectors and search readiness;
reprocessing changed Claude transcripts updates their derived data normally.

Claude message UUIDs are scoped to their native session and agent, independent
of the transcript's physical path. SQLite retains each physical source's
contribution and derives one logical thread from their union. Matching copies
are deduplicated; divergent branches retain their own causal context. Rewinding,
emptying, or deleting one copy removes only that copy's contribution. Conflicting
payloads or ancestry for the same native message UUID fail transactionally.
Repeated metadata events with conflicting parents are ignored when no retained
dialogue depends on them; ambiguous events in a message's parent chain still fail.
Compaction may append reduced copies of earlier dialogue with rewritten parents.
Mash retains the original archived ancestry only for an uninterrupted block of
existing UUIDs followed by a native compaction boundary whose logical parent and
preserved tail identify the preceding event outside that block. Replayed message
content must still match; unrelated reparenting and incomplete replay evidence
fail transactionally. The native compaction boundary remains a context root.
Native forks may retain the original session ID on their copied history. When
the new session's dialogue explicitly links into that history, the whole copied
transcript belongs to the fork's native session; the original remains a separate
thread. Nested forks follow the same rule. Unlinked transitions and interleaved
session IDs fail rather than silently combining unrelated histories. Records
without a UUID retain path-based fallback identity.

Normal and independently routed shared publication use this same reconciliation.
The first upgrade of older routed Claude checkpoints requires an explicit
`mash index --all-history` when a bounded history horizon would prevent replaying
retained contributions. Changing old path-based message IDs can require
re-embedding Claude messages; unrelated providers' vectors remain intact.
Source snapshots increase SQLite storage, especially with many physical copies.

For pipeline diagnostics, set `MASH_TRACE=1`. mash writes JSON trace events to
stderr, including process/request IDs, request elapsed milliseconds, nested stage spans, span busy/idle timing,
safe error kinds, and HTTP status codes. CLI JSON and MCP messages remain on
stdout. Only mash's own trace targets are enabled; dependency logs are excluded.
Trace fields omit query/history text, paths, filters, credentials, and vectors.
Existing indexing progress events can also appear on stderr.

Qdrant publication spans separate snapshot, catalog and document staging from
the conditional head write, head verification and cleanup. The publication
outcome distinguishes observed success, unresolved writes, conflicts, staging
failure and verification failure. Cleanup reports whether a deletion failed;
trace fields contain no node/group IDs, backend URLs or staging versions.

Index traces separate discovery and checkpoint filtering, parser admission waits,
parsing retries, queue sends, application, embedding planning and wave reads,
CPU/Core ML inference batches, vector writes, and commits. Admission events
include the estimated reservation, budget, elapsed wait, and whether capacity
forced a wait. Inference events report batch counts and Core ML sequence shape;
they never include the texts or vectors. Request context follows parser and CPU
inference threads through the same propagation helper.

For example, `MASH_TRACE=1 mash search cbioportal --local --no-worker 2>trace.jsonl`
exposes model loading, embedding, lexical retrieval, vector loading/scoring and
reranking. Use `MASH_TRACE=1 mash mcp` for repeated requests or
`MASH_TRACE=1 mash serve --server-config server.toml` on the shared server. The on-demand
worker suppresses its own stderr to preserve detached execution. With tracing
enabled, its reply carries a validated opaque `trace_id`, worker process ID,
execution milliseconds (excluding connection/queue wait), and outcome. The CLI
emits these as a `worker reply` event inside its `worker_search` span. The reply
also carries up to 15 fixed search-stage summaries, emitted as `worker stage`
events with the same trace ID. Each reports `calls`, `total_us` and `max_us` for
model loading, embedding, lexical/vector retrieval and scoring, shared HTTP,
and reranking when those stages run. These are inclusive span lifetimes, not
CPU time; nested and concurrent timings must not be added together. Summaries
arrive with the final reply. Use `--no-worker` or MCP for full span events.
Tracing is selected per worker request and untraced requests omit diagnostics.
Request IDs are
local to a process, paired with process IDs. With tracing enabled, each shared
HTTP call sends an opaque `x-mash-trace-id`; its `shared_http_request` client
span and the server's `http_request` span record the same `trace_id`. Enable
tracing on both processes and match that field to correlate status and timing.
The server replaces missing, malformed or duplicate IDs with a fresh local ID;
client tracing disabled sends no correlation header or worker diagnostics request.
Successful shared response bodies are decoded into typed models on blocking
workers so large JSON responses do not occupy an async runtime worker. The
`shared_http_decode` child span records byte count, `mode` and success/error, with the
same request ancestry and no response contents. HTTP request timing includes
admission, network I/O, blocking-worker queueing and decoding. In `buffered` mode
the child measures parsing; in `streamed` mode it also includes waiting for body
frames. Streamed byte counts include reader read-ahead. The request span records `queue_ms`, the fixed `endpoint` name and
`admission` class. Each process admits up to eight interactive requests and one
bulk state/publication request. They use separate capacity, acquired before
sending HTTP. The existing 2-second interactive or 600-second bulk deadline
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
See the [response decoding checks](benchmarks/2026-10-03-shared-response-decoding.md)
for concurrency and relevance verification.
The [response admission checks](benchmarks/2026-10-03-shared-response-admission.md)
cover queue expiry, cancellation, separate capacity and remaining memory limits.
The [streaming control](benchmarks/2026-10-03-shared-response-streaming.md)
documents the large-state memory/parsing tradeoff and body cancellation checks.
Live worker stage streaming and fleet overhead measurements remain planned.

Aider is opt-in through `[providers.aider]` with `roots = ["/absolute/project"]`
or an explicit Markdown history file. Directory roots discover only
`.aider.chat.history.md`; custom `--chat-history-file` paths can be configured
directly. Aider's native format prefixes user lines with `####` and operational
output with `>`; code fences are preserved. It does not record a timezone,
per-message timestamps, or reliable repository metadata, so those fields remain
unknown. Its unlabelled Markdown can also make assistant blockquotes ambiguous;
outside code fences these remain context-only, consistent with operational output.
Each history file is one thread, including appended Aider runs. See the
[upstream history writer](https://github.com/Aider-AI/aider/blob/main/aider/io.py).

Hermes Agent is opt-in through `[providers.hermes]`, with
`roots = ["/absolute/path/to/.hermes/state.db"]` or its containing directory.
The adapter reads native SQLite sessions without running upstream migrations or
repairs. Each session becomes a thread; compaction copies are deduplicated and
model-only rows are excluded. User and assistant prose is searchable, while tool
calls and results remain canonical context. Logged `cwd` and `git_repo_root`
populate project/repository filters. WAL-only changes invalidate checkpoints and
trigger watchers; the history horizon uses each session's activity time.
The adapter requires the native display-history columns in the
[upstream schema](https://github.com/NousResearch/hermes-agent/blob/main/hermes_state_common.py)
and follows its [display projection](https://github.com/NousResearch/hermes-agent/blob/main/hermes_state_messages.py).

OpenCode is opt-in through `[providers.opencode]`, with `roots` pointing to
`opencode.db` (normally `~/.local/share/opencode/opencode.db`) or its containing
directory. It reads sessions and project metadata from native SQLite snapshots,
including WAL changes. Each session becomes one thread. Sessions with
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

OpenClaw is opt-in through `[providers.openclaw]`, with `roots` pointing to
per-agent `openclaw-agent.sqlite` databases (normally
`~/.openclaw/agents/<agentId>/agent/openclaw-agent.sqlite`) or a directory
containing them. Each native session window becomes a thread. The adapter reads
plain and Zstandard-compressed events and verifies cold archives stored in SQLite
or the adjacent `sessions/cold` directory, without restoring or changing the
upstream database. Current branch ancestry and the latest reset window control
visible history; compaction retains earlier visible messages. Explicitly retained
reset messages stay visible. Prose is searchable; tools, reasoning, summaries and
visible extension messages remain context only. Hidden extension messages are
omitted. Logged `cwd` supplies the project filter; repository metadata is absent.
WAL and cold-file changes invalidate checkpoints and trigger watchers. Missing or
invalid archives/navigation fail indexing instead of replacing history with an
empty result. The native compression columns and cold-archive format follow the
[upstream schema](https://github.com/openclaw/openclaw/blob/main/src/state/openclaw-agent-schema.sql)
and [archive codec](https://github.com/openclaw/openclaw/blob/main/src/config/sessions/session-cold-storage-codec.ts).

Pi is opt-in through `[providers.pi]`, with
`roots = ["/absolute/path/to/.pi/agent/sessions"]` or explicit `.jsonl` session
files. The adapter reads native v3 sessions and indexes every branch in one
thread, storing each message once. Metadata entries preserve parent relationships
without becoming searchable. Tools, reasoning, compaction/branch summaries and
visible custom messages remain context only; hidden custom messages are omitted.
Context edits affect Pi's model input and do not delete original searchable history.
Logged `cwd` supplies project metadata; no repository metadata is inferred.
Older session versions fail explicitly; open them in Pi to run its own migration.
The format follows Pi's [session manager](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/src/core/session-manager.ts).

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
`copilot` provider filter and per-index allowlists. Their upstream schemas are
the [VS Code session store](https://github.com/microsoft/vscode/blob/f5f73df304d9c5eeab64d4d07eed57e007742a92/src/vs/workbench/contrib/chat/common/model/chatSessionStore.ts)
and [typed mutation schema](https://github.com/microsoft/vscode/blob/f5f73df304d9c5eeab64d4d07eed57e007742a92/src/vs/workbench/contrib/chat/common/model/chatSessionOperationLog.ts).

Gemini CLI is opt-in through `[providers.gemini-cli]`, with
`roots = ["/absolute/path/to/.gemini/tmp"]` or explicit native `.jsonl`/`.json`
session files. Configure its sandbox runtime directory instead when Gemini
stores sessions there. The adapter replays native recording semantics: repeated
message IDs replace previous versions in their original position, `$set.messages`
replaces the saved history, and `$rewindTo` removes its target and everything
after it. A checkpoint introducing the native `<state_snapshot>` summary and
its exact acknowledgement preserves earlier directly recorded dialogue for
search. Synthetic summaries and newly copied context stay outside search.
Other replacement checkpoints remain authoritative, including cancelled turns.
Removing the compaction anchor clears its archived dialogue. Unmarked summaries
cannot be distinguished safely from arbitrary context replacements, so those
still follow Gemini's resumed history. Broken rewind references and incomplete writes
fail explicitly and preserve the previous indexed snapshot.

Search uses `displayContent` when present and keeps thoughts, tools, commands,
hook/session context, operational messages and subagent sessions out of search
evidence. Newly introduced checkpoint turns are model context; previously recorded
turns keep their provenance across checkpoint updates. Complete JSON snapshots
have no event provenance, so filtering there uses the stored roles and known
context markers. Tool and reasoning text remains available through `view`; binary
attachment contents are omitted. The adjacent native `.project_root` ownership
marker supplies project metadata only when its SHA-256 matches the logged
`projectHash`; otherwise the logged hash identifies the project. Marker creation,
changes and removal trigger a refresh without changing the history's age horizon.
Repository metadata is not inferred. A JSONL sibling takes precedence over its
old JSON file when both are discovered, avoiding duplicate migrated sessions.
Reads are limited to 8 MiB per JSONL record or complete native JSON document,
and 8 KiB per project marker. A structural check caps JSON records at 65,536
nodes before typed decoding. Replay retains normalized text, including recovered
compacted dialogue, with a 32 MiB
accounting budget and at most 65,536 message IDs; replacements, checkpoints and
rewinds release that budget. These are parser bounds rather than process RSS
limits. Oversized input is reported without truncation and preserves the previous
indexed revision.
The format follows the official [recording schema](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/chatRecordingTypes.ts),
[recording service](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/chatRecordingService.ts)
and [project registry](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/config/projectRegistry.ts).

Cline is opt-in through `[providers.cline]`. Configure the extension's native
`tasks` directory, explicit `ui_messages.json` files, the SDK `sessions.db`, or
directories containing native `*.messages.json` artifacts. The current SDK
defaults to `~/.cline/data/db/sessions.db`; its configured directories and
recorded artifact paths can differ. The adapter reads the catalog and its WAL
without modifying them, follows recorded message paths, and suppresses duplicate
artifact discovery when the catalog owns those files.

UI snapshots preserve task requests, feedback, assistant prose and typed
questions/plans. Partial responses, reasoning and operational rows remain
context-only. SDK artifacts preserve repeated real turns despite IDs/timestamps
being regenerated on resume. Visible completion summaries and replies to known
question tools are searchable; ordinary tools, attachments, injected model
context, hidden resume prompts and subagent histories are context-only.
Compaction sidecars do not replace canonical message history. Logged native
task metadata, SDK manifests or catalog workspace roots supply project filters;
repositories are not inferred from files read by the model.

Snapshots are limited to 8 MiB and each decoded message/block sequence to 16,384
entries. Invalid or interrupted writes preserve the previous indexed revision.
Metadata and external artifact changes invalidate source checkpoints and are
included in watch targets. When both extension and SDK roots are configured,
histories with the same task ID retain the legacy UI prefix and append SDK turns
after Cline's native resume warning. Without that boundary, SDK history alone
is authoritative. Ambiguous multiple histories of the same kind are rejected.
See the source-linked [Cline contract](docs/cline-ingestion-contract.md) for
verified schemas and remaining validation gates.

UI and SDK user-input envelopes are retained in canonical viewing and removed
from search projections. Schema 35 repairs older Cline projections; run explicit
`mash index` to rebuild any changed vectors before searching. Unchanged vectors
are preserved; search never performs the upgrade itself.

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
is limited to 8 MiB and 65,536 nodes; each conversation has a 64 MiB read budget
and 32 MiB retained-history budget. IDE and Agent identities remain distinct.
Its [native storage contract](docs/cursor-ide-ingestion-contract.md) records the
fixture provenance and remaining captured-session validation gates.
The [Cursor ingestion contract](docs/cursor-ingestion-contract.md)
records first-party fixture provenance and remaining real-session validation gates.

Windsurf (including the current Devin Desktop Cascade format) is opt-in through
`[providers.windsurf]`. Configure its native `~/.codeium/windsurf/cascade`
directory or explicit `<trajectory-id>.pb` files. Mash reads the encrypted
protobuf files directly; the editor and its language server need not be running.
Archive markers and temporary files are not separate conversations. Saved step
positions distinguish repeated requests and preserve prefix IDs across snapshot
rewinds. User queries and assistant responses enter search; thinking, modified
response context, copied steps and system/model-generated user prompts do not.
Native workspace and Git-root file URIs supply project/repository filters.

Each encrypted snapshot is limited to 8 MiB, decoded protobuf fields to 65,536,
and normalized retained history to 32 MiB. Authentication failures, malformed
payloads, mismatched identities and oversized input fail indexing and preserve
the previous revision. Background trajectories and unknown provenance receive
no search projection; nested subtrajectories and tool-step payloads are not
currently reconstructed. The [native contract](docs/windsurf-ingestion-contract.md)
records first-party reader/writer fixture evidence and remaining actual UI-session,
subtrajectory and rewind validation gates. These are parser limits, not aggregate
process RSS guarantees.

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

Each native BLOB is limited to 8 MiB, each conversation to 64 MiB of reads,
65,536 steps/decoded fields and 32 MiB of retained normalized history. Unknown
step formats, SQL/protobuf disagreements, malformed timestamps and oversized
input fail explicitly and preserve the previous index revision. Native workspace
and Git-root URIs supply project/repository filters when unambiguous. The
[Antigravity contract](docs/antigravity-ingestion-contract.md) records native
writer/fork evidence and remaining real UI, successful rewind, CLI/Hub and
cross-version validation gates. These limits do not establish aggregate RSS.

For tree histories, `view --context N` includes up to N actual ancestors and
up to N messages along the first saved continuation from the match. It never
adds sibling replies because their file ordinals happen to be nearby. Contextual
reranking follows the same rule on SQLite and Qdrant. Linear histories
keep their existing ordinal neighbourhoods. SQLite migrations preserve existing
messages and vectors. Update the shared service before
publishing Pi, Copilot, Gemini, Cline, Cursor, Windsurf or Antigravity histories from upgraded nodes.

Codex histories include raw response records and completed `UserMessage` /
`AgentMessage` turn items. Nearby raw/completed counterparts are paired once;
unmatched dialogue and separate repeated requests are retained. Started items,
streaming deltas and legacy event mirrors are omitted. See the
[native Codex contract](docs/codex-ingestion-contract.md) for migration fixtures
and reconciliation limits. Existing nodes require an explicit `mash index`
refresh for schema 32 / `search-text-v6`; search does not migrate or ingest.

## Configuration

The rename changes the default configuration and data directories to `mash`,
and the database filename to `mash.sqlite3`. To reuse an existing achi installation,
pass its paths with `--config`, `--db`, and `--models-dir`, or move its configuration,
database, and installed models into the new locations reported by `mash status`.

mash reads `config.toml` from the platform mash configuration directory. Use
`mash status` to see the resolved path, or pass `--config PATH` explicitly.
The file is strict, versioned TOML. Provider presence enables that provider;
roots must be absolute. `index.since_days` defaults to 90. Set
`index.all_history = true` instead for all history; these settings cannot be
combined. CLI indexing flags override these settings.

Optional `[gold.query]` settings select the default `unit` (`"threads"` or
`"messages"`) and `limit` (1–200). Omitting them preserves threads and 20 results.
CLI/MCP search parameters override either setting without rebuilding the index.
`[gold.query.fusion]` controls hybrid candidate fusion independently of gold
builds: `rank_constant` defaults to 60 (1–1000), and `lexical_weight` and
`semantic_weight` each default to 1 (1–100). Higher weights favor that candidate
stream before contextual reranking. These query-only settings apply to local,
combined and fallback searches, take effect on the next request, and require no
index rebuild. Exact-mention search is independent of fusion settings.
Set `[gold.query] ranking = "fusion"` to keep fused hybrid ordering and skip
contextual reranking and its context fetches. The default is
`"contextual-rerank"`. This query-only policy applies across local, shared and
combined searches through the same CLI/MCP core, including worker requests.
It changes no stored vectors or silver. Dense query embeddings still require
the local embedding model or configured remote service. Fusion skips loading
the reranker. Local installation inventory validation remains required; remote
fusion needs no local model installation. Status reports `query_models` as
`"available"` or `"unavailable"` for current TOML query defaults,
separately from physical `models_installed`. Switching ranking reloads the
matching cached backend.
Use diagnostics to compare execution and results before choosing a default.
For one search, use `--ranking fusion` or MCP `"ranking":"fusion"`; the other
choice is `contextual-rerank`. Omission inherits current TOML. Explicit ranking
overrides require related-discussion intent, do not persist, and never rebuild
the index.
For one hybrid request, CLI `search` accepts `--fusion-rank-constant`,
`--lexical-weight` and `--semantic-weight`; MCP `search` accepts a partial
`fusion` object with the corresponding underscore-named fields. Omitted fields
inherit current TOML defaults. Overrides are validated before model work,
apply to local/shared/fallback ranking, and are rejected for exact-mention.
For resolved policy and retrieval outcomes, use CLI `search --diagnostics` or
MCP `search` with `"diagnostics":true`. The optional report includes selected
artifacts, completed retrieval sources, fallback, result count and core time.
It does not run indexing or change search results.
See [silver enrichment and gold configuration](docs/silver-gold-configuration.md)
for the layer contract and current configuration scope.

```toml
version = 1

[providers.claude-code]
roots = ["/home/james/.claude/projects"]

[providers.codex]
roots = ["/home/james/.codex/sessions"]

[index]
since_days = 90
destination = "both"

[index.local]
providers = ["claude-code", "codex"]

[index.shared]
providers = ["codex"]

[shared]
group_id = "personal"
node_id = "xenia"
api_url = "https://mash.example.net"
token_env = "MASH_NODE_TOKEN"
```

This example keeps both providers in SQLite and forwards only Codex to the shared service.
SQLite is required on every node. `index.destination` accepts `local` or `both`;
`both` requires the top-level `shared` connection settings. Without an explicit
destination, mash selects both when shared settings exist, and local otherwise.
The local provider allowlist must contain every provider in the shared allowlist.
Shared-only configurations are rejected.

Each destination's optional `providers` list is an allowlist of enabled providers.
Omit a destination filter to include all enabled providers; `providers = []` sends
none to that destination. `index --provider` further restricts these lists and
cannot bypass them. Filters control subsequent ingestion; narrowing them does
not automatically erase previously indexed history. Use `forget` for deletion.

Omit the top-level `shared` for a fully local installation. Search defaults to SQLite plus other nodes in the shared group when shared settings exist; otherwise it uses SQLite. Configure the same
group ID and API URL on every member, with a distinct node ID and token for each
publisher. The service authorizes each token for a fixed group and node. Group IDs select
separate Qdrant collections; every node within a group contributes to one joint
index. The database can live wherever you choose.

The former `nodes`, `local_node`, SSH destinations, `--local-node`,
and hidden federation workers are removed. To migrate a configuration, move this
machine's `providers` and `index` settings to the top level and optionally add
`shared`. Old configuration shapes are rejected rather than guessed.

## Search filters

The current models support English queries. Unsupported scripts and confidently
detected non-English queries return a usage error asking for an English query,
before loading models or opening an index. Short names and ambiguous technical
terms remain searchable; language detection cannot reliably classify every short
or mixed-language query.

CLI search accepts repeatable `--provider`, `--node`, `--role`, `--conversation`,
`--project`, and `--repository`. Repeated values for one field are alternatives;
different fields are combined with AND. `--node` filters publishing identity and
does not connect directly to another node. A standalone local installation uses
the node label `local`. `--days` is a rolling message-age window. `--after` is
inclusive and `--before` exclusive; both accept `YYYY-MM-DD` (UTC midnight) or
RFC3339 timestamps with an explicit offset.

MCP exposes the same selection fields: `provider`, `conversation`, `node`,
`role`, `project` and `repository` are arrays, including for a single value.
`after` and `before` use Unix seconds. Omitted or empty shortcut arrays impose
no restriction; an empty list inside a structured predicate, such as
`{"conversation": []}`, matches nothing. Unknown thread IDs match nothing.
Use canonical `conversation_id` values returned by thread search to drill down
into messages from promising threads:

```bash
mash search 'deployment' --unit threads
mash search 'rollback procedure' --unit messages \
  --conversation THREAD_A --conversation THREAD_B \
  --provider codex --provider claude-code
```

The corresponding MCP drill-down request is:

```json
{
  "query": "rollback procedure",
  "unit": "messages",
  "conversation": ["THREAD_A", "THREAD_B"],
  "provider": ["codex", "claude-code"]
}
```

```bash
mash search 'database migration' --provider codex --provider claude-code \
  --role user --role assistant --after 2026-09-01 --before 2026-10-01
mash search 'deployment' --repository https://github.com/kojah/mash.git \
  --node dev-macbook --node xenia
```

For Boolean combinations, use `--filter` with a structured JSON expression, or
the MCP search tool's `filter` argument. This is a query expression; configuration
files remain TOML. Each expression contains exactly one operator:

- `all`: AND of expressions; an empty list matches everything.
- `any`: OR of expressions; an empty list matches nothing.
- `not`: negation of one expression.
- `node`, `provider`, `role`, `conversation`, `project`, `repository`: lists of exact, case-sensitive values.
- `after`, `before`: inclusive/exclusive Unix timestamps **in seconds**.

An MCP request can select either repository while excluding tool messages:

```json
{
  "query": "database migration",
  "filter": {
    "all": [
      {"provider": ["codex", "claude-code"]},
      {"any": [
        {"repository": ["https://github.com/kojah/mash.git"]},
        {"project": ["/home/james/scratch/mash"]}
      ]},
      {"not": {"role": ["tool"]}}
    ]
  }
}
```

The CLI accepts the same expression through `--filter '...JSON...'`. Convenience
flags and the structured expression are combined with AND. Filters apply before
lexical/semantic candidate limits on SQLite and Qdrant, then the
combined results are reranked. Raw SQL, regular expressions, and glob patterns
are not supported. Limits are eight levels of nesting, 128 expressions, and 100
values per field list.

Project means the session's first recorded working directory. Repository means
the session's logged Git repository URL, or the locally available Git origin for
that directory at ingestion time. They are conversation metadata; the first
directory applies to the whole session. Unavailable metadata remains unknown,
so a positive filter does not match it and a negated filter does. Date filters do
not match messages without timestamps. Schema upgrades preserve history and
embeddings and invalidate local ingestion checkpoints, allowing the next
`mash index` to capture metadata from source files. Independently routed shared
ingestion also refreshes its source checkpoints when upgrading metadata support.

## Shared index

Nodes access the shared index through an authenticated Axum HTTP service. Only
that service connects to Qdrant. There is one
joint index per group, with node ownership enforced by the service.

`POST /v1/threads` lists thread metadata without model inference, message bodies
or vectors. Use the same bearer token as other shared API calls. Its typed JSON
request accepts `provider` and `node` arrays, literal ASCII-case-insensitive
`title_contains`, inclusive `after` and exclusive `before` activity bounds in
Unix seconds, and `exclude_self`. Activity refers to recorded thread `updated_at`;
unknown activity does not match a time bound. Omitted or empty arrays impose no
restriction. Sort by `updated_at` (default), `created_at` or `title`, with
`direction` set to `asc` or `desc` (default). Title ordering uses binary string
order. Missing sort values always come last; provider, thread ID and node break
ties deterministically.

The response contains `threads` with canonical `conversation_id`, provider,
representative `node_id`, title/project/repository and timestamps in Unix
milliseconds, plus `next_offset`. Copies with the same provider/thread ID are
deduplicated before paging; the best matching copy for the requested sort order
supplies metadata. `limit` defaults to 50 (1–100); `offset` defaults to 0
(0–10000). Pagination stops before advertising an offset beyond 10000; narrow
filters to browse larger collections. Pages are stable while the index is unchanged; updates between page
requests can shift boundaries.

Use `mash list-threads` or the MCP `list_threads` tool for the same metadata
discovery. These thin wrappers call the shared application core. The default
scope combines local SQLite and other nodes in the configured shared index;
`--local`/`local=true` stays offline, and `--shared`/`shared=true` reads only the
shared index including this node. Local and remote reads run concurrently, with
deduplication and ordering applied before the final page. A shared read has a
two-second total deadline; combined discovery returns local metadata with
`shared_error` on failure, while shared-only discovery fails. `shared` in the
response indicates a successful shared read.

SQLite metadata reads require an existing current schema and never create or
migrate the database. They require no installed models or completed embedding
generation, so metadata already recorded during indexing can be inspected.
This does not make semantic search ready. MCP sort values use `updated_at` and
`created_at`; CLI values use `updated-at` and `created-at`.

For example, orient first and then search within a returned thread:

```bash
mash list-threads --provider codex --title-contains cbioportal --limit 10
mash search 'database migration' --unit messages --provider codex --conversation THREAD_ID
```

The HTTP service uses [Qdrant](#qdrant-shared-storage). Configure `qdrant_url_env`;
there is no backend selector. PostgreSQL/Redis settings are rejected. An existing
Qdrant configuration must omit `backend = "qdrant"` before upgrading.
See the [Qdrant-only transition](docs/qdrant-only-transition.md) for validation
and the separate live-data migration work.

Each node has a different randomly generated token (at least 32 printable ASCII
bytes), supplied through its configured `shared.token_env`. Server group membership
comes from the `nodes` list; restart to change membership or rotate tokens.
A node can search its whole group and can only publish or forget its own data.
Database credentials belong on the HTTP server, supplied through environment variables.
Use an HTTPS reverse proxy for remote access or HTTP on a trusted private network.
The service binds to `127.0.0.1:8787` by default. Node clients disable redirects.

The API exposes `/v1/index` (NDJSON), `/v1/state`, `/v1/status`, `/v1/freshness`,
`/v1/search`, `/v1/threads`, `/v1/view`, and `/v1/forget`. Requests use bearer
credentials. Search carries a query embedding and returns candidates for node-side
fusion and reranking. The HTTP server needs no model assets. Interrupted or invalid
uploads leave the visible revision and freshness unchanged. Uploads are bounded
by 128 records per batch, 8 MiB per event, 8 GiB per request, and the configured
`qdrant_max_node_mib` (64 MiB default). Ordinary JSON requests are limited to 64 KiB.
Node/revision checks reject stale uploads; full snapshot retries are idempotent.

Set `max_concurrent_uploads` (default 4, range 1–16) to bound concurrent staging
work; large histories should use 1. The same capacity also covers `forget`
mutations. A complete upload or accepted forget operation
keeps its slot until the work succeeds or fails, including if the requester
disconnects or times out. Cancellation does not imply rollback: retry using the
existing node/revision checks. Busy mutations return a retryable HTTP 503;
search and metadata reads do not consume mutation slots. This bounds the number
of mutations, not their total RAM.

Qdrant uses `max_mutation_snapshot_mib` (default 1,024, range
1–65,536) to admit snapshot work by size. Each mutation reserves the sum of its
existing canonical snapshot and incoming upload bytes, rounded up to MiB, with
a minimum of one MiB. `forget` reserves the existing snapshot size. Reservations
are shared across all configured nodes/groups in one API process and remain held
through decoding, planning and publication. Busy capacity returns a retryable
503. A mutation larger than the entire budget returns a nonretryable 400 naming
the setting to increase; the active revision stays unchanged. Uploads and stored
chunks may already have been privately spooled before this check, but typed
snapshot decoding has not started.

This setting measures serialized inputs, **not RSS**. Allow additional RAM for
decoded objects, readers, allocator caches, transport buffers and database staging;
multiple API processes have separate budgets. Raising a node's payload cap may
also require raising this budget, especially for full replacements that count
both the old snapshot and new upload. The [two-node admission comparison](benchmarks/2026-10-01-shared-snapshot-budget.md)
records the memory/throughput tradeoff and successful retries on a real corpus.
The `reserve_snapshot_work` trace span reports work MiB, limit MiB and
accepted/busy/oversized outcome without payloads or node identifiers.

The [large-replacement overlap measurement](benchmarks/2026-10-01-shared-publication-concurrency.md)
records candidate-search latency and API/database memory with one mutation slot;
it does not validate four simultaneous large publications or a 2-GiB database budget.
Qdrant context reads use projected thread ancestry before fetching selected
message bodies; the [long-thread context comparison](benchmarks/2026-10-01-qdrant-context-projection.md)
records the measured memory/latency change and remaining thread-size costs.
Staging changed revisions uses extra memory and writes,
and the service loads canonical node data when applying changes; unchanged data
can reuse the existing stage. These are deliberate limits on the initial
backend. Benchmark actual search latency, update cost, memory use, and retrieval
quality for a group. See [the measured comparison](benchmarks/2026-09-30-shared-index.md).

In both mode without destination filters, local indexing commits first. Changes
mark a durable local publication revision;
a changed node stages its complete snapshot in Qdrant and atomically advances its head. Raw tool outputs are uploaded
for context but remain excluded from search. Unchanged revisions are acknowledged
without replacing shared records. A failed upload returns local success with
`shared.state: "pending"`; subsequent `index` runs retry even with no new history.
`status` checks publication state locally; `status --shared` contacts the API.

`forget` deletes a locally owned conversation and publishes a persistent
shared tombstone. Check its `shared.state`: an offline deletion remains pending
until a later successful `index` or `forget`. Copies owned by other nodes remain.
In local/both modes, node identity is tied to the local database instance; a
recreated database must use a new node ID. Stale snapshots cannot replace newer
accepted revisions.

With per-index filters in both mode, SQLite ingests only the local allowlist and
the shared publication coordinator independently ingests the shared allowlist from the original history
files. Qdrant stores the shared source checkpoints. Local work still commits
first and shared failures return pending; retry with `index` while retaining the
original shared history files. Shared forget intents are durably queued in SQLite
when offline and applied before the next shared ingestion. A successful forget
can leave index status pending until `index` catches up other changes.

Default search queries SQLite and shared records belonging to other nodes. The
shared service excludes the authenticated node before candidate limits and
embedding-readiness checks. After computing one query embedding, local candidate
retrieval and the shared HTTP request run concurrently. Both sources use independent
lexical/semantic reciprocal-rank fusion, followed by bounded combined reranking.
Local message hits have no `node_id`; shared hits include their
publishing node for `view --shared NODE_ID`. Plain `view` always reads SQLite.
Interactive shared reads have a two-second HTTP deadline. Fetching nearby shared
context has a one-second total budget, preserving completed responses and using
available message evidence for the rest. Transport failures appear in shared
search status; combined search retains local results. Streaming publication has
a separate ten-minute budget.

Search defaults to `unit = "threads"` in CLI and MCP. Each thread result includes
its conversation ID, provider, representative node, available title/project/repository
and conversation timestamps, a ranking score, and up to three distinct matching
excerpts in relevance order. Each excerpt has a `message_id`, its own `node_id`,
role, timestamp, and relevance scores. Open an excerpt with `view MESSAGE_ID
--context N`, supplying `--shared NODE_ID` for a shared excerpt. Timestamps are
Unix milliseconds; unknown metadata is null. Search never returns an entire
conversation automatically.

Local lexical search treats query text as literal words, including URLs, dotted
names and hyphenated identifiers; double quotes require contiguous literal words
in each returned message or excerpt. Punctuation
does not become SQLite FTS syntax. Queries must contain a letter or number and
fit within 16,384 bytes. Explicit PR/issue references require that exact reference
in each returned excerpt/message, so `PR 180` cannot be replaced by `PR 183`
or an incidental numeric value of 180. PR and issue references remain distinct;
a complete reference URL also constrains its host and repository.
Use the metadata filters for Boolean scopes.
See [the search quality audit](docs/search-audit-2026-09-30.md) for measured failures
and remaining relevance limitations.

Thread retrieval considers up to 200 lexical and 200 semantic candidates per
index. It keeps up to six distinct candidate messages per conversation. A first
reranking pass evaluates up to 20 grouped thread documents, then individual
evidence is reranked for five to ten promising threads (at most 60 messages).
Each evidence document includes bounded text from its two nearest searchable
neighbours. Threads are grouped before applying `limit`;
their score is reciprocal rank of the best qualifying message plus a bounded
bonus for up to two additional qualifying excerpts. The bonus is smaller than
the gap to the next message rank, so weaker matches cannot push a thread above
one with stronger primary evidence. Scores express relative relevance, not probabilities.
Lexical matches and verified exact PR/issue references remain eligible outside
the reranking budget. Other semantic-only
matches qualify only after being evaluated by the reranker; unreranked neighbours
do not fill out thread results. Raw model logits are not calibrated probabilities,
so a negative score alone does not disqualify a semantic paraphrase. A query may
return fewer threads than requested. Message mode applies the same evidence gate.
Recency does not change ranking; use date filters to restrict history.
Copies with the same provider and conversation ID are grouped across nodes;
identical role and whitespace-normalized message content earns no additional credit.
Replicas are collapsed before fusion and thread scoring; each excerpt's `copies`
lists alternate message/node references for viewing the same evidence. Independent conversations
with different IDs remain separate.

Use `mash search QUERY --unit messages` or MCP `unit = "messages"` for full
individual message results with the existing 50-candidate and ten-message
reranking budgets. Copies with the same provider, conversation, canonical message
ID, role and whitespace-normalized content occupy one message result, with
alternate view anchors in `copies`. Repeated content with different message IDs
and divergent copies remain separate. Replicas do not earn additional fusion
credit. `limit` counts the selected unit, and filters apply equally
to both modes before retrieval. Thread excerpts are bounded to 800 characters
plus ellipses, with a window around a query term when present. Results describe
the bounded candidate pool, not an exhaustive count of matches in each chat.
Unknown upstream metadata remains null.

Reranking uses batches of up to four documents grouped by length, reducing
inference overhead and padding between short and long candidates.
For messages longer than 2,400 characters, reranking uses a bounded window around
the first query term (or the start for semantic-only matches), keeping late lexical
evidence visible instead of always truncating the message's opening. `view` still
returns the canonical message.
Search projections exclude system/developer/tool messages, dedicated skill bodies,
and known injected instruction/context blocks. Ordinary Markdown examples remain
searchable. `view` retains canonical provider history. Existing candidates are
also filtered at query time; the next explicit `mash index` rescans source files
to refresh stored projections and titles. Shared-only indexing similarly refreshes
its source checkpoints once. Search never performs that refresh itself.

Use `search --local` to avoid the shared API, or `search --shared` for the complete
shared index, including this node. MCP search exposes equivalent `local` and
`shared` booleans. A candidate API failure during default search returns local results
with `realized_mode = "local-hybrid"` and `shared_error`; explicit shared search
returns an error. Later context failures retain the retrieved shared evidence,
`shared = true`, and the merged/shared mode, and report `shared_context_error`
separately. These source flags work independently of opt-in diagnostics.
Incomplete local embeddings still require `mash index`.
Models remain local and are installed only by `models install`.
Installation metadata is checked before loading models: the marker is limited to
1 MiB and 4,096 asset entries, revisions to 128 bytes, and each tokenizer/config
file to 16 MiB. Invalid paths, duplicate entries, wrong local dimensions and
missing or truncated assets return a model error recommending `mash models install`.
Hugging Face snapshot symlinks remain supported. Markers are published atomically
after installation checks, using a unique staging file for each installer.
Only one installer can run against a model directory at a time. A second
`models install` returns a retryable `model-busy` error (exit code 6); retry
after the active installer finishes. The operating system releases the lock
when the installer exits, so an interrupted process does not leave a stale lock.
Installers copy available assets into a separate generation, run SDK checks
there, then atomically select that generation. Failed or interrupted staging
keeps the previous installation available. Searches and indexers retain a lease
on their selected generation; an explicit installation collects abandoned and
retired generations once no reader holds them. The original flat cache is
retained so readers started before the first generation install can finish.
Reinstallation needs disk space for a second copy of the assets. This staging
also isolates mutable SDK revision references from the active cache.
Model downloads always use mash's selected model directory, even when `HF_HOME`
is set for another application. Set `--models-dir` to change mash's model location;
`HF_ENDPOINT` remains available for a Hugging Face mirror.
On Unix, installers flush the asset files and directories before activation,
then flush the marker's directory entry after activation. If that final flush
fails, the retryable error states that the installation is active but its
durability is unconfirmed. Both generations are retained; retry `models install`.
Activation errors also retain prepared assets until a later confirmed cleanup.
Other platforms retain atomic activation without these Unix flush barriers.

For unfiltered both mode, full snapshot replacement favors correctness and
simplicity over upload throughput. Large histories incur a full transfer on each changed revision, and
exact retrieval scores the matching shared vectors. Qdrant also supports opt-in HNSW.
Qdrant serializes canonical publication JSON to private temporary
files on blocking workers and stream bounded chunks from those files. This
avoids keeping a second full serialized snapshot in API memory. Temporary disk
must hold the incoming upload and canonical JSON
until publication finishes. The validated canonical data still resides in
memory. Stored canonical snapshots are also received into
private, checksummed files and decoded on blocking workers, avoiding a full
input JSON buffer during replacement or retry. Temporary disk must accommodate
these reads too. Backend staging and validated data have additional costs;
the configured node snapshot limit is not a process RAM budget. Account for
concurrent uploads when provisioning the API and database.
Shared-service provisioning remains explicit. Measure corpus size, latency and
recall tradeoffs before choosing approximate retrieval.

### Qdrant shared storage

Configure `qdrant_url_env` on the HTTP service. Nodes continue using the same
API URL, bearer token, CLI and MCP tools. Qdrant **1.19+ in the 1.x series** is
required for conditional node publication; older versions are rejected before
creating collections. Exact vector retrieval remains the default; HNSW is opt-in.

```toml
version = 1
listen = "127.0.0.1:8787"
qdrant_url_env = "MASH_QDRANT_URL"
# Optional for an authenticated Qdrant service:
# qdrant_api_key_env = "MASH_QDRANT_API_KEY"
qdrant_max_node_mib = 64
qdrant_memory = "cached" # Or "cold" for vectors and dense/sparse indexes without prewarming.

[[nodes]]
group_id = "personal"
node_id = "xenia"
token_env = "MASH_XENIA_TOKEN"

[gold.query.vector_search]
algorithm = "exact"
# To opt into approximate retrieval instead:
# algorithm = "hnsw"
# ef_runtime = 200

# Optional default for a configured group:
# [gold.query.groups.personal]
# algorithm = "hnsw"
# ef_runtime = 80
```

The server's `gold.query.vector_search` controls retrieval from the existing
dense index. Defaults are persisted per group. Use `mash serve --server-config
server.toml --activate-query` to intentionally change existing defaults; conflicting
startup settings otherwise fail. Running API processes read the selected policy
on subsequent hybrid searches. This does not rebuild vectors, change gold build
identity, or modify silver. The previous
top-level `qdrant_vector_index` table is rejected; move it to the table above.
Groups inherit the server default unless named in `gold.query.groups.GROUP`.
Status exposes the selected policy in `group_gold.vector_search`. Node-side
`gold.query` still controls intent, fusion and result shape independently.

```bash
docker run --name mash-qdrant -p 127.0.0.1:6333:6333 \
  -v mash-qdrant-data:/qdrant/storage qdrant/qdrant:v1.19.1
export MASH_QDRANT_URL='http://127.0.0.1:6333/'
# Set MASH_XENIA_TOKEN to the node's existing API token.
mash serve --server-config /absolute/path/to/qdrant-server.toml
```

Shared gold settings are persisted per group. Startup with conflicting build/model
settings fails. To deliberately replace the selected settings, update the server
TOML and start with `--activate-gold`; rebuild each contributing node with
`mash index --rebuild-shared-gold`. Old API settings fail closed, and dense search
requires compatible manifests across the group. This preserves canonical history
but interrupts dense search during replacement. See
[the activation contract](docs/silver-gold-configuration.md#durable-shared-group-configuration)
for publication races and remaining lifecycle gates.

Use an HTTPS origin and `qdrant_api_key_env` for hosted Qdrant. Endpoints cannot
contain credentials, paths, queries or fragments; redirects are disabled so keys
are not forwarded elsewhere. Qdrant credentials belong only on the HTTP server.
Populating a new shared group requires an explicit `mash index` from each publishing node.
`qdrant_memory` selects the Qdrant 1.19+ `cached` (default) or `cold` tier for
dense vectors, the HNSW index and the sparse lexical index in new message
collections. Startup validates reported placement on existing collections;
changing the setting requires a new group and explicit reindexing. Mash does
not automatically migrate or rebuild collections. Cold storage still uses the
operating system's page cache, and payload indexes and snapshot publication have
their own memory costs. Lower peak RAM and search latency need workload-specific
measurement; see [Qdrant's storage documentation](https://qdrant.tech/documentation/manage-data/storage/).

Each group has an isolated message collection with named cosine dense vectors
and IDF-weighted sparse lexical vectors, plus a metadata collection for canonical
snapshots, checkpoints, tombstones, and thread catalogs. Lexical retrieval uses
lowercase alphanumeric tokens, hashed sparse term IDs, saturated term frequency,
and a fixed 256-token length reference; exact token predicates prevent hash
collisions from producing false lexical matches. This is a backend-specific
lexical ranking function; it differs from SQLite FTS5 ranking.
The existing application core still fuses, reranks, and groups message candidates.
All expressive filters apply inside Qdrant before candidate limits. HNSW is
approximate; exact mode stays available without rebuilding the collection.
Quantization is disabled. Existing incompatible dimensions, vector configuration,
or payload index types are rejected without deleting data.

Uploads stage immutable versions and atomically update a group catalogue with
expected configuration, catalogue and node versions. Activation updates that same
point, fencing delayed publications. Competing stale writers receive a conflict;
different-node updates may retry metadata publication up to three times. Reads only admit active versions
and verify their heads before returning. Canonical snapshots are checksummed and
chunked; thread listing reads metadata without message bodies or vectors. The
64 MiB limit applies to incoming uploads and each canonical node snapshot and is
configurable. Keep Qdrant storage persistent and backed up. Obsolete versions are
removed after publication; failed cleanup or a server crash can leave hidden
staging data on disk, which does not appear in search results.

## Remote embeddings

Local FastEmbed inference remains the default. Add `embeddings` to the node
configuration to use Voyage-4 for both document and query embeddings:

```toml
[embeddings]
api = "voyage"
url = "https://api.voyageai.com/v1/embeddings"
model = "voyage-4"
dimensions = 1024
token_env = "VOYAGE_API_KEY"
```

Set `VOYAGE_API_KEY` in the node's environment. It is separate from the shared
index token; the index service never receives it. History search projections
and search queries are sent to the configured inference endpoint. There is no
automatic fallback to another model. `model` is configurable, including
`voyage-code-4`; Voyage dimensions can be 256, 512, 1024, or 2048. Requests use
`input_type: document` for indexing and `input_type: query` for searching.
Overlong inputs fail explicitly rather than being silently truncated.

For an OpenAI-compatible endpoint, set `api` to `openai-compatible` and provide
its complete embeddings URL, model, dimensions, and optional `token_env`.
Set `request_dimensions = true` if the endpoint should receive the `dimensions`
parameter. Fixed-dimension self-hosted endpoints can omit it. Mash supports
1–2048 dimensions and validates vector count, ordering, dimensions, and values.
HTTP requests are asynchronous, bounded, and time out after 60 seconds. Local
SQLite pipelines remain on blocking workers while remote requests run on Tokio.

Remote indexing needs no local model assets. Reranking remains local; run
`mash models install` with remote embeddings configured to install only the
reranker before searching. No models are downloaded by indexing or searching.
If a valid local embedding installation already exists, installing for remote
embeddings preserves it. Switching back to local inference does not require
reinstalling those assets; the changed embedding space still requires indexing.

For shared indexing, configure the service's embedding space as well:

```toml
[embeddings]
model = "voyage-4"
dimensions = 1024
```

The service configuration applies to every group it serves. Publishers and
searchers must use the matching model and dimensions. Changing them rebuilds
local embeddings on the next index pass; reindex all shared publishers before
searching. Qdrant collections have fixed dimensions: use a new group ID when changing dimensions,
then reindex its members. Model identifiers and dimensions prevent mismatched
queries from being compared with the stored embedding space.

## Automatic synchronization

Run `mash index --watch` for opt-in synchronization while the process remains
running. It indexes once at startup, then waits for one second of quiet after
history changes before refreshing. Set `--debounce-ms 500` to change that delay
(50–60000 ms). Continuous writes trigger a pass within ten seconds, or the
configured delay if longer. Passes run sequentially; changes during indexing
queue another pass. Existing provider, age, destination, and per-index allowlist
settings still apply. `--full` cannot be combined with `--watch`.

The watcher uses native `notify` filesystem events, ignores file reads and
unrelated files, and notices JSONL creation, modification, replacement, and
deletion. It reloads configuration on each pass and watches for config/model
marker changes. Missing roots are watched through their nearest existing parent.
Removing an authoritative configured root reports an error rather than purging
its history; removing individual sources from an existing root is reconciled.
Failed indexing and pending shared publication retry after 30 seconds without
requiring another file change. A reconciliation pass every five minutes recovers
missed events. No models are downloaded automatically; every node retains SQLite.

Each source read captures a fresh stamp and retries detected changes up to three
times, with 50 ms between attempts. Local and shared-only ingestion both use this
stable-read check. Stable malformed input fails without retries. If a source keeps
changing, indexing returns a retryable `source-changed` error recommending `index`;
an incomplete scan cannot authorize removal of missing histories. Local queued
results are checked again before writing. If the source changed while waiting,
the stale content is discarded and a new bounded stable read runs before any
content or checkpoint is replaced. A removed source still requires another
indexing pass.

Watch mode writes one JSON result per completed pass to stdout (newline-delimited
JSON); progress and typed errors go to stderr. Ctrl-C finishes the active pass
and exits. For MCP, use `mash mcp --watch` with optional `--debounce-ms`; watching
lasts for the MCP session and shares the tool mutation/model concurrency guards.
Its refresh results go to stderr so stdout remains MCP protocol only.

Run `mash status --shared`, or call the MCP `status` tool with `{"shared":true}`,
to inspect `shared_index.nodes` for every publishing node in the configured
group. Each node includes its revision, conversation/message counts, and UTC
timestamps:

- `last_synced_at`: last successfully committed indexing/publication check,
  including a check with no new data.
- `last_updated_at`: last committed data update or deletion. A check with no
  changed data preserves this timestamp.

The API records these timestamps in the atomically published Qdrant node head.
Failed uploads leave them unchanged. They describe received
index data, not the newest source message's timestamp or proof that a node is
currently online. Older nodes can have null timestamps until they publish with
this version. Use `mash status --shared` to include shared-index freshness;
ordinary status remains usable offline. Freshness inspection is
read-only.

## MCP

CLI and MCP are wrappers around the same typed application core in `app/core.rs`.
The core owns indexing, search, thread listing, view, status, forgetting, and model installation.
MCP calls it directly without constructing CLI arguments or invoking CLI handlers.
The cached search worker and watcher also use typed core requests.

`app/core/models.rs` defines those serialized requests and responses.
Operation handlers live in `app/core/indexing.rs`, `search.rs`, `threads.rs`, `inspection.rs`
and `maintenance.rs`; each returns its concrete result type. The façade resolves
configuration and paths, dispatches the request and wraps the result once.
Indexing owns per-index provider routing, progress and watcher target selection;
inspection owns read-only status and canonical views. Parsing, SQLite and local
inference run on blocking workers, while shared HTTP operations remain asynchronous.

`app/config.rs` owns the typed node settings and status models. Its private
`document` module decodes strict TOML and validates provider routing, roots,
identifiers and endpoints; `loading` handles bounded file reads and platform
defaults. CLI, MCP, watcher and worker operations use the same resolved settings.

`app/shared.rs` is the typed shared-index facade. Its private `client` module
owns pooled HTTP requests, authentication, deadlines and response decoding;
`publication` owns durable acknowledgements, routing ownership and streaming
uploads; `retrieval` owns concurrent local/shared candidates, fusion and contextual
ranking. The HTTP service and concrete database backends remain separate from
those node-side modules. These boundaries use existing concrete types rather
than adding transport or provider abstractions.

`app/shared/server.rs` owns Axum routing, authorization, typed HTTP responses
and the listener lifecycle. Its private `configuration` module loads bounded
strict TOML and resolves authorized node identities; `store` initializes Qdrant; `upload` spools bounded
NDJSON streams. Configuration and credentials are validated before connecting
to storage, and backend initialization completes before the listener opens.

Run `mash mcp` as a stdio MCP server. It accepts `--config`, `--db`, and
`--models-dir`, `--watch`, and `--debounce-ms` at startup and exposes `search`, `list_threads`, `view`, `status`, `index`, `forget`,
and `models_install`. The server uses the same operations and typed JSON errors
as the CLI; tool failures set MCP `isError`.

To stream an `index` tool call's progress, include a string or numeric
`params._meta.progressToken` in the `tools/call` request. The server sends
standard `notifications/progress` with the same token while the job runs,
including when it waits for another mutation. `message` contains the phase,
counts, elapsed time, and phase ETA. Structured progress fields are also available
under `params._meta["dev.jamesqo.mash/index-progress"]` in each notification.
The numeric `progress` is an increasing update counter across all phases;
`total` is omitted because the whole job's work is unknown. Phase-specific
totals and `eta_seconds` are in the structured metadata. Unknown ETAs are `null`.

Progress delivery uses a bounded queue and skips intermediate updates if a
client is slow; a disconnected client does not block indexing. The normal
tool result follows progress and retains the CLI's success/error structure.
Without a progress token, the tool returns only its final result. CLI progress
still goes to stderr; MCP stdout remains exclusively SDK protocol messages.

Example MCP client configuration:

```json
{
  "mcpServers": {
    "mash": {
      "command": "/absolute/path/to/mash",
      "args": ["mcp", "--config", "/absolute/path/to/config.toml"]
    }
  }
}
```

For shared operations, provide the configured node token environment variable to
`mash mcp` through your MCP client's environment mechanism. `search` accepts a
`shared` boolean and a `unit` of `threads` (default) or `messages`; `view` accepts a `shared` publishing node ID. Search, view, and
status and `list_threads` are annotated read-only. Index and forget mutate data; `models_install`
is the only operation that downloads models. Shared node I/O uses async HTTP;
service-side Qdrant I/O uses async HTTP. SQLite, parsing, and local FastEmbed inference run on blocking
workers and are awaited by callers. MCP mutations serialize per session; status
and view may run concurrently, while model-heavy operations use a single permit
to bound memory and CPU use. Remote embeddings use the same node configuration
for CLI and MCP operations.

## Code organization

CLI and MCP translate inputs into typed application-core requests. Implementation
modules are grouped by responsibility:

- `app/silver/`: canonical models, typed provider records, history discovery,
  normalization and source reconciliation; `app/silver.rs` coordinates ingestion
  and checkpoints. Provider-owned history files are bronze inputs.
  `app/silver/dialogue.rs` interprets visible dialogue independently of retrieval;
  canonical messages retain full context alongside an optional `dialogue_text`.
- `app/silver/local.rs` and `app/gold/local.rs`: local stage boundaries for
  durable canonical ingestion and rebuildable retrieval state. The application
  core wires those stages together.
- `app/gold/shared.rs`: node-side embedding and publication-payload materialization
  from interpreted silver conversations; source scanning stays in the shared
  publication coordinator.
- `app/semantic/` and `app/search_*`: gold model execution, embedding batches,
  Core ML, remote embeddings, hybrid retrieval and query/ranking policies.
- `app/shared/routed.rs`: HTTP publication coordination for per-index provider
  routing; source interpretation stays in silver and materialization in gold.
- `app/shared/snapshot/`: backend-independent publication validation; `layers.rs`
  defines persisted silver history and gold embeddings within one atomic revision.
- `app/shared/qdrant_index/`: typed REST, conditional publication, and dense/sparse retrieval.
- `app/tests/`: CLI contract tests grouped by behavior, with common setup in
  `support.rs`. CLI and MCP share marker fixtures from `app/test_support.rs`.

Keep assertions in individual tests. Share setup for repeated fixtures, and
split independent cases so each failure names the behavior that broke.

## Verification

Use Cranelift for development builds and tests. Install the nightly component
and use the Cargo aliases:

```bash
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
cargo build-clif
cargo run-clif -- status
cargo test-clif
```

These aliases select Cranelift for mash and mash-query in the development profile. Dependencies
keep LLVM, so their existing build cache can be reused. `test-clif` requires
`cargo-nextest`. Use the usual commands below for standard LLVM verification
and `cargo build --release` for optimized builds. Cranelift is an experimental
nightly component and is available only on supported host platforms; see the
[Cranelift backend documentation](https://github.com/rust-lang/rustc_codegen_cranelift#usage).

```bash
cargo fmt --check
cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo test --doc
```

The handwritten workspace includes the application and the pure `mash-query`
crate. These default commands cover both. For an isolated query edit cycle,
use `env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif -p mash-query`.
See [crate boundaries](docs/crate-boundaries.md) for responsibilities and
the integrated checks required when changing search behavior.

For repeated unit-test edits, keep one target directory and enable incremental
compilation explicitly:

```bash
env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --lib
# Add a Nextest expression to run the affected unit tests:
env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --lib -E 'test(claude_copies::)'
```

On Xenia, warm incremental rebuilds after a small test edit took 7.7–8.6 seconds,
compared with approximately 32–35 seconds without incremental compilation.
The first incremental build took 41.7 seconds and created additional disk cache.
The empty `RUSTC_WRAPPER` bypasses a locally configured compiler wrapper; the
installed wrapper failed on this incremental build. `--lib` selects unit tests
and does not run CLI integration contracts. Keep the full suite, real-model and
shared-backend checks as completion gates. See the
[measured workflow](benchmarks/2026-09-30-test-turnaround.md) for scope and limits.

CI also runs the [labeled hybrid relevance matrix](benchmarks/2026-10-01-shared-relevance.md)
with explicitly installed models against local SQLite and all three shared backends.
The shared tests require their disposable backend endpoint and
`MASH_TEST_MODELS_DIR`. To run these checks together with those variables set:

```bash
cargo test-clif --lib --run-ignored only -j 1 -E 'test(labeled)' --success-output immediate
```

The installed August 2026 nightly can hit a [Clippy compiler crash](https://github.com/rust-lang/rust/issues/161495)
when checking async code. On that toolchain, use
`RUSTFLAGS=-Znext-solver=no cargo clippy --all-targets -- -D warnings`.

The storage integration tests require disposable Qdrant 1.19+ in
`MASH_TEST_QDRANT_URL`. Tests start real HTTP services and create/remove isolated
collections for their owned groups:

```bash
MASH_TEST_MODELS_DIR='/path/to/explicitly-installed-models' \
  MASH_TEST_QDRANT_URL='http://localhost:6333/' \
  cargo nextest run --run-ignored only --no-tests fail -j 2 -E 'test(qdrant)'
```

The real-model integration tests are ignored by default and require
`MASH_TEST_MODELS_DIR` pointing at assets created by `mash models install`.
The shared-backend selections include Claude copy reconciliation with real
embeddings and require that directory too. Selected tests fail when prerequisites
are missing; `--no-tests fail` rejects an accidentally empty selection.

For an instrumented coverage report, install
[cargo-llvm-cov 0.9.1](https://github.com/taiki-e/cargo-llvm-cov/releases/tag/v0.9.1)
and the toolchain's `llvm-tools-preview` component. Set the model directory and
the Qdrant test URL above to installed models and disposable services.

Also set `MASH_TEST_SERVER_BINARY` to the instrumented `mash` binary produced
in the coverage target's `debug` directory. With an explicit `CARGO_TARGET_DIR`,
use `$CARGO_TARGET_DIR/llvm-cov-target/debug/mash`; without it, the default is
`target/llvm-cov-target/debug/mash`. An explicit `CARGO_LLVM_COV_TARGET_DIR` instead
uses that directory directly, as defined by [cargo-llvm-cov 0.9.1](https://github.com/taiki-e/cargo-llvm-cov/blob/v0.9.1/src/cargo.rs#L63).
The suite builds the binary before running tests, so it need not exist before
the coverage command starts. Restart contracts
require Docker and the cached Qdrant image; set `MASH_TEST_DOCKER_SG=1` on Xenia.
They own their containers and API processes and do not restart the supplied
backend test services. Use the instrumented binary; forced API termination can
prevent its final LLVM profile flush, so the contract's runtime checks do not
guarantee that all child-process execution appears in the coverage report.

Then run:

```bash
rustup component add llvm-tools-preview
make coverage
```

This runs every test except the manual benchmarks, including the installed
model and all three shared-backend contracts. It writes LLVM JSON coverage to
`target/llvm-cov/coverage.json`; set `COVERAGE_OUTPUT` to choose another location.
The coverage task cleans workspace coverage artifacts before execution so older
binaries cannot contribute obsolete source mappings in a persistent target.
It waits for idle workers from that target to exit normally before cleaning and
reporting, allowing their coverage profiles to be written without changing
production worker behavior. On nightly, use `CARGO_ARGS=--branch` to include
LLVM's experimental branch instrumentation. This does not measure MC/DC.
Generated SQL bindings, separate test files, standalone test helpers and the
test-only relevance evaluator, offline topic prototypes and manual benchmark
modules are excluded from the report. The topic module is compiled only for tests;
its experiments still run in the suite but cannot pad application coverage.
Tests embedded in production source files still contribute to those files'
coverage, so assess uncovered behavior as well as the aggregate percentage.
Coverage needs LLVM instrumentation; regular development continues to use
Cranelift. On the affected nightly, use `RUSTFLAGS=-Znext-solver=no make coverage`.

To compare inference thread counts and reranking batch sizes on your own corpus,
use an explicitly refreshed, isolated index and installed models:

```bash
MASH_TEST_MODELS_DIR=/path/to/installed/models \
MASH_TEST_SEARCH_DB=/path/to/refreshed/index.sqlite3 \
  cargo test-clif --release --run-ignored all -j 1 \
  -E 'test(benchmark_real_search)' --success-output immediate
```

These manual diagnostics open the index read-only, warm models, and emit JSON
timings for seven queries in message and thread modes. They compare ordered
message IDs and thread excerpt IDs against the default inference configuration;
the batch comparison also checks scores. Ranking changes fail the diagnostic.
Repeated settings help reveal timing noise. Measurements exclude CLI startup,
worker transport and shared search, and do not establish relevance by themselves.
Use these alongside end-to-end measurements before changing production defaults.

CPU indexing reserves two workers for long embedding batches and allows short
batches to use all eight workers. It preserves the dynamically quantized model's
batch membership and vector generation. See the
[indexing memory measurements](benchmarks/2026-09-30-index-memory.md) for the
memory/throughput tradeoff and the manual worker-budget diagnostic. This is an
inference workspace limit, not a total indexing memory limit.


## Automatic deployment

Every push to `main` first runs CI, including generated-query checks, linting,
and SQLite, real embedding/reranking/hybrid-search, and Qdrant tests.
CI explicitly installs and smoke-tests local models on cache misses; model assets
are cached by platform, dependency lockfile and installation implementation.
The shared-storage gate includes normal and routed Claude-copy reconciliation.
Only a successful run can build and
deploy the Linux x86-64 and Apple Silicon binaries from that same commit.
Deployments are serialized; a superseded commit is skipped after acquiring
the deployment lock. New pushes do not interrupt an active deployment.
To request a manual deployment, run the **CI** workflow on `main`; it runs
the same validation and current-commit gate. Pull requests and manual runs
on other branches never deploy.

Deployment atomically installs `mash` to `~/.local/bin` on Xenia,
dev-macbook, and personal-macbook over Tailscale. Atomic installation is per
machine; a network failure can leave the fleet partially updated, and the
workflow reports failure so the current CI deployment can be retried.
The macOS binary is
cross-compiled on the Linux runner with `cargo-zigbuild`; destination machines
need neither this repository nor a Rust toolchain.

Deployment requires `TS_OAUTH_CLIENT_ID`, `TS_OAUTH_SECRET`,
`DEPLOY_SSH_PRIVATE_KEY`, and `DEPLOY_SSH_KNOWN_HOSTS` repository secrets.
After the current-main check, a preflight verifies that all four are populated
before starting either platform build. Missing credentials fail deployment with
the missing secret names; credential values are never printed by this check.
Successful CI alone does not confirm that any machine was updated: verify the
deployment job and destination smoke checks too.

## License

See [LICENSE](LICENSE).
