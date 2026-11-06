# Keeping history up to date

[Documentation](README.md) · [Project home](../README.md)

The history plugin's SessionStart hook starts a detached, low-priority
incremental `mash index` at most every ten minutes and never waits for it, so
new sessions find a recent index; `MASH_AUTO_INDEX=off` disables it. A pass that
finds the database busy (another index or a watcher) exits without changes.

Run `mash index --watch` for opt-in synchronization while the process remains
running. It indexes once at startup, then waits for one second of quiet after
history changes before refreshing. Set `--debounce-ms 500` to change that delay
(50–60000 ms). Continuous writes trigger a pass within ten seconds, or the
configured delay if longer. Passes run sequentially; changes during indexing
queue another pass. Existing provider, age and destination settings still apply. `--full` cannot be combined with `--watch`.

Only one watcher indexes each database at a time, across all processes. Every
watching process (`index --watch`) first takes
an exclusive advisory lock on `<database>.watch-lock` beside the canonical
database path. A process that cannot take it becomes a standby: it neither
watches files nor indexes, and retries the lock every second. The operating
system releases the lock when the leader stops or exits, including a crash; the
standby that takes it over starts with a full initial pass, so changes made
while no process was watching are indexed. The leader's settings win: a
standby's `--debounce-ms` applies only once it leads. CLI watch mode reports each
role change as a `{"event":"watch-role","role":"leader"|"standby"}` line on
stderr. Explicit `index` calls do not take the lock.

The watcher uses native `notify` filesystem events, ignores file reads and
unrelated files, and notices JSONL creation, modification, replacement, and
deletion. It reloads configuration on each pass and watches for config/model
marker changes. Missing roots are watched through their nearest existing parent.
Removing an authoritative configured root reports an error rather than purging
its history; removing individual sources from an existing root is reconciled.
Failed indexing and pending shared publication retry after 30 seconds without
requiring another file change. A reconciliation pass every five minutes recovers
missed events. No models are downloaded automatically; every node retains SQLite.

Watchers use one local CPU embedding worker instead of the eight used by explicit
indexing. They keep the same embedding batches and vector format, trading bulk
throughput for lower memory use. The indexing model pool is reused during bursts
and released after 60–90 seconds without model work; the next update reloads it.
Idle release also applies to Apple Silicon's Core ML indexing pool. Allocators may
retain freed memory, so releasing the pool is not a process RSS limit. Search model
caching is separate from the indexing pool.

Each source read captures a fresh stamp and retries detected changes up to three
times, with 50 ms between attempts. Local ingestion, including histories subsequently published to the shared
service, uses this stable-read check. Stable malformed input fails without retries. If a source keeps
changing, indexing returns a retryable `source-changed` error recommending `index`;
an incomplete scan cannot authorize removal of missing histories. Local queued
results are checked again before writing. If the source changed while waiting,
the stale content is discarded and a new bounded stable read runs before any
content or checkpoint is replaced. A removed source still requires another
indexing pass.

Watch mode writes one JSON result per completed pass to stdout (newline-delimited
JSON); progress and typed errors go to stderr. Ctrl-C finishes the active pass
and exits. MCP does not watch history and rejects `--watch`, `--debounce-ms`, and
the retired `watch_start`, `watch_stop`, and `watch_status` tools. Run the CLI
watcher separately; all MCP sessions reading that database see its committed
updates without reconnecting. Without a watcher, refreshes come from explicit CLI/MCP indexing or the
plugin's throttled session-start indexing pass. No daemon is installed.

Local and combined searches include `freshness_warnings` in JSON and MCP text
when no watcher holds the database's leader lock, watcher activity cannot be
checked, or configured providers have incomplete/unverified scan evidence.
These warnings do not block results or start indexing. Shared-only searches do
not inspect the caller's local watcher. A running watcher and a completed scan
do not prove that current provider files are indexed: this lightweight check
does not crawl history, detect newly created files, or report a last-success
timestamp or historical indexing error. Check the watcher's output for failures.
To suppress these warnings (enabled by default, no rebuild required):

```toml
[views.query]
freshness_warnings = false
```

This switch does not suppress schema incompatibility errors. MCP returns those
as errors in both text and structured output, with reconnect/upgrade guidance
for newer silver schemas and export/import guidance for older ones.

Long-lived MCP and CLI watch processes register locked records beside the
selected database in `<database>.runtimes`. Installation diagnostics list their
PIDs, running revisions, invocation paths and whether those executables still
match. This inventory covers registered processes for that database only; older
releases and other databases are outside its scope. A fresh MCP probe does not
verify an existing agent session.
An explicit MCP probe also registers its short-lived child; its record is removed
on exit, but an empty runtime directory may remain. It never indexes or downloads
models.
An unchanged executable from a different release is reported as `different_build`;
it must be reconnected explicitly because its own invocation path did not change.

After executable replacement, MCP rejects new mutations when they acquire the
session's mutation guard; CLI watchers check before each pass and once per second
while idle or standing by. Already admitted operations may finish. Reconnect MCP
or restart the CLI watcher after upgrading. This is an admission check, not
transaction fencing against an installer racing a check. Read-only MCP tools
remain available, but use the running release's schema and contract.

Run `mash status --shared`, or call the MCP `status` tool with `{"shared":true}`,
to inspect `shared_index.activity`: service-wide occupied/capacity counts and
live operations belonging to your authenticated group. Each operation reports
its node, kind, phase, elapsed time, phase counters, and time since its last
reported progress. Counters reset at phase boundaries and do not certify durable
commits. Long SQLite statements can run without intermediate updates; elapsed
time alone does not prove a stall. Detached server work remains visible after
its requester disconnects. Completed work disappears; server restart resets the
inventory. Older servers omit this field, which means unknown.
The `checking_search` phase checks collection health once and probes retrieval
after awaited document writes. Normal background optimization does not delay
publication: no collection-wide optimizer drain is required. Failed or paused
optimization and failed probes still return errors. Certification describes
applied search data, not an idle optimizer or a guarantee of query latency under
load.

When mutation slots are occupied, up to 32 authenticated requests wait in FIFO
order for at most five minutes before returning a retryable capacity error. Waiting
is reported as `waiting_for_capacity`, separately from receiving an upload or
waiting for the node's writer lock. The handler does not collect upload bodies
until admitted. Queue overflow is rejected immediately. Dropping a waiting
handler releases its queue position; disconnected sockets may remain waiting
until the server observes cancellation or the deadline expires. Once work is
admitted and detached, it retains its slot through completion. This bounds
waiting and active work independently; it does not increase publication capacity.

With `MASH_TRACE=1`, shared-operation start, phase transitions and end events go
to structured server logs, correlated by operation ID. Events include node IDs,
timings and enum phases, never history contents or credentials. Mutation outcome
events distinguish success from failure. Status is a point-in-time observation;
it does not queue work, increase concurrency or retry mutations.

Use that same status response
to inspect `shared_index.nodes` for every publishing node in the configured
group. Each node includes its revision, chat/message counts, and UTC
timestamps:

- `last_synced_at`: last successfully committed indexing/publication check,
  including a check with no new data.
- `last_updated_at`: last committed data update or deletion. A check with no
  changed data preserves this timestamp.

The API records these timestamps in the node's shared silver head, inside the
SQLite transaction that applies the upload.
Failed uploads leave them unchanged. They describe received
index data, not the newest source message's timestamp or proof that a node is
currently online. Older nodes can have null timestamps until they publish with
this version. Use `mash status --shared` to include shared-index freshness;
ordinary status remains usable offline. Freshness inspection is
read-only.

`shared_index.unpublished_nodes` lists configured members of the authenticated
group that have no retained publication, in sorted order. An empty array means
every member configured on this API has published; an omitted field means that
membership scope was not supplied. Published nodes still require separate
lexical/dense readiness and provider-coverage checks. This reflects the API's
current credential configuration, not network discovery, liveness or proof of
complete source coverage; historical publications remain in `nodes` even if
their credentials are later removed.
