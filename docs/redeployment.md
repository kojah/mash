# Redeployment and index refresh

Build one Linux and one Apple Silicon artifact from the validated commit, then
reuse them across the nodes. The development MCP deploys selected targets in
order, checks API prerequisites and atomically replaces each binary. It does
not ingest history. Configure the shared API before dependent history nodes;
the trusted restart helper preserves publication identities and starts the API
in a separate session so it survives deployment-client disconnection.
Existing MCP processes keep their running binary; reconnect clients to use the
installed release. Restart the shared API separately using its existing server
configuration and credentials, preserving Qdrant data.

The local and remote install helpers skip binary replacement when the executable
already matches the artifact (byte comparison locally, SHA-256 remotely). Remote
installs check before transferring the artifact. Builds and readiness checks still
run; an identical executable does not certify index readiness or source policy.

## Decide whether an index refresh is required

Run `mash status` with the node's normal configuration/database/model paths.
When the indexing destination is `both`, also run `mash status --shared`.
Read the typed status fields rather than deciding from the release commit alone.

| Finding | Action |
| --- | --- |
| Configured local artifacts are ready and the shared publication is current | Install/reconnect and verify search; skip indexing. |
| Query models are unavailable | Use explicit `models install` or fix the selected inference configuration. Indexing cannot repair missing assets. |
| Source interpretation, provider policy or privacy migration changed | Run explicit ingestion. Retained old metadata is not evidence of a completed migration. |
| Local gold is stale but certified current silver is retained | Use `index --rebuild-local-gold` when its certificate allows it. |
| Shared gold alone changed and current shared silver is certified | Use `index --rebuild-shared-gold` instead of scanning provider files again. |
| Publication is pending or this node is absent from shared freshness | Run explicit indexing/synchronization once the API is available. |
| Node/API is offline | Record the pending deployment/publication; preserve local search and the staged artifact for retry. |

Check `ready`, `local_artifacts`, `query_models`, `silver`, `shared.state` and
the configured node's shared freshness (`gold_current`, `lexical_current`,
`artifacts`, `source_silver`). A ready default query does not certify every
optional representation; inspect the configured artifacts too. A successful
HTTP health check does not prove search or synchronization is current.

Skipping a deployment refresh does not certify that provider files are unchanged.
The opt-in watcher or an explicit `index` command still owns ongoing ingestion.
Do not use `--full` as a routine deployment step. Make database backups when a
database migration or data move requires them, rather than copying the entire
database on every binary update.

## Required shared publication

The API admits a bounded number of mutations. Upload node snapshots in sequence
during a planned fleet refresh. A newly visible publication head does not prove
the previous writer has released its slot: reclamation can continue afterward.
The publication-plan GET retries HTTP 503 with exponential backoff capped at
five seconds, within the existing 30-minute bulk request deadline. Waiting emits
`shared-waiting` progress through the CLI and MCP sink. Authorization errors,
conflicts and mutation POSTs are not automatically replayed.

Awaited Qdrant point writes and filtered deletions use the same bulk deadline;
database reads retain their 30-second deadline. A completed backend write can
take more than 30 seconds during full-node staging. Timeout does not prove that
the write was rejected, so publication verification owns ambiguous-write
recovery rather than blindly replaying a mutation.

After committing and reclaiming obsolete versions, publication waits for the
message collection's optimizer queue and running jobs to drain before returning
success. Collection health must also be green with a healthy optimizer. This
wait belongs to indexing, is bounded to five minutes, and revalidates the active
gold configuration before acknowledging completion. Small, already settled
publications need no polling delay. Publications with retained embeddings also
run a payload-free exact probe over all visible group versions followed by a
probe of the configured HNSW path, with a 30-second budget per probe. Interactive
search uses HNSW by default and retains its five-second deadline.

The publication remains committed if the optimizer fails, pauses, cannot be
inspected or exceeds that budget; failure does not undo acknowledged database
writes. Inspect shared freshness and backend health before another ingestion
attempt. Searches during staging/optimization and other concurrent publications
can still contend for backend resources. See [the cold-search investigation](cold-search.md).

Qdrant catalogue/document staging uses at most 512 points and approximately
2 MiB of encoded point bytes per request, whichever fills first. A single larger
valid point is sent alone; the existing 30 MiB hard transport allowance remains.
Each typed point is serialized once into retained JSON bytes. Every write still
requires `wait=true`, strong ordering and a completed operation. The conditional
publication commit remains the visibility boundary; batching introduces no
in-place edits to active data.

## Why the previous refresh was slow

The October 4 `d05812e` dev MacBook refresh required a provider/source migration:
434,507 messages, 68,731 forwarded embeddings and no new embedding inference.

| Stage | Measured time |
| --- | ---: |
| Optimized Linux build | 120 s |
| Cached native Mac build | 53 s |
| Provider ingestion | 19.5 s |
| Snapshot preparation | 92.3 s |
| 1,163,622,360-byte upload | 284.0 s |
| Qdrant commit, including staging/reclamation | approximately 580 s |
| Complete dev index operation | 978.2 s |

These are one rollout's stage measurements, not latency guarantees. The old
32-point staging limit required about 13,600 message-write requests for that
node. Larger bounded batches address this request overhead; they do not remove
snapshot preparation, network transfer or optimizer work.

The live rollout report is outside Git at
`/home/james/scratch/mash-artifacts/redeploy-d05812e-20261004/deployment-report.json`.
Personal MacBook was offline and was not deployed in that rollout.

## Bounded-batch comparison

October 4 measurements used Qdrant 1.19.1 over loopback HTTP, the same copied
25-thread/6,137-message corpus, and the same installed local models. Initial
publication forwarded 1,061 retained embeddings with no inference. Its 28
immutable provider-file copies were revalidated; no live provider files or
indexes were changed. Scan certificates may advance during that revalidation;
canonical history hashes matched across both runs and all rebuilds. Shared
source identities remained unchanged during gold-only rebuilds.

| Complete operation | Previous 32-point batches | Bounded 512-point batches |
| --- | ---: | ---: |
| Initial publication | 39.30 s | 4.81 s |
| Rebuild shared lexical gold | 11.66 s | 3.19 s |
| Rebuild shared dense gold | 51.86 s | 72.88 s |
| Document staging requests, all three operations | 576 | 38 |

All 20 paired search responses matched exactly, covering `cbioportal` and
`qdrant`, filtered/unfiltered requests, and exact/semantic intents where the
representation allowed them. The dense rebuild performed 1,061 inferences;
embedding work took 42.01 s before and 39.29 s after. Its remaining work was
slower despite fewer staging requests.

These are single observations on a busy Linux host, with integration jobs on a
separate Qdrant instance sharing the physical disk. Optimizer/reclamation and
I/O timing varied. The evidence establishes fewer awaited writes and equivalent
responses on this sample; it does not establish uniform latency improvement,
fleet throughput, or a new search-quality result. A 128-point variant was not
profiled. Private captures and the adapted, pinned benchmark harness live in
`/home/james/scratch/mash-artifacts/redeploy-speed-20261004`.

## Incremental publication

SQLite records changed thread IDs and their publication revisions in the same
transaction as conversation, message, enrichment, embedding or tombstone writes.
Each upload captures a consistent read transaction and sends complete replacements
for changed threads, plus explicit deletions. Acknowledgements prune only accepted
revisions; concurrent newer edits remain pending. A durable journal floor forces
a full capture when change history is unavailable or the destination changes.
Recipe/model changes and stale shared artifacts also require full capture.

Qdrant stores immutable per-thread snapshots, catalogue rows and search documents.
The conditional group catalogue atomically selects active thread versions. Unchanged
threads retain their points; replacement/deletion excludes old versions before
ranking. After verified publication, native Qdrant filtered deletion reclaims
obsolete versions. Ambiguous writes retain staging for recovery. There is no
custom database log or segment compactor. Old full-node snapshots remain readable
and transition on their next publication. Install the new API before publishing
from upgraded nodes; older API binaries cannot interpret partitioned publications.

Schema 42 installs the journal without rebuilding schema-41 history or discarding
vectors. Read-only commands require the current schema: perform the explicit
index/schema upgrade once before validating an upgraded node. MCP `index` is also
the explicit refresh tool; subsequent searches read refreshed data without
restarting the MCP process. Rebuild flags distinguish retrieval rebuilding from
provider ingestion, and `_meta.progressToken` enables streamed progress.

October 4 comparison used identical copies of the 25-thread/6,137-message corpus,
Qdrant 1.19.1 with disk-backed Docker storage over loopback, installed local models,
and the previous bounded-batch release. Controlled context-only additions affected
one thread and required no inference. Addition timings include the explicit CLI
index operation; deletion timings cover the HTTP forget request. Write counts are
Qdrant point-upsert requests, including the conditional catalogue update, excluding
reads, filtered cleanup requests and asynchronous optimizer work.

| Operation | Previous time | Incremental time | Previous upload | Incremental upload | Point writes, previous → incremental |
| --- | ---: | ---: | ---: | ---: | ---: |
| Initial publication | 3.39 s | 3.89 s | 13.38 MB | 13.38 MB | 19 → 23 |
| Add 1 message | 4.08 s | 0.50 s | 13.38 MB | 6.49 KB | 19 → 4 |
| Add 10 messages | 4.76 s | 0.50 s | 13.39 MB | 9.29 KB | 19 → 4 |
| Add 100 messages | 3.82 s | 0.48 s | 13.42 MB | 37.70 KB | 19 → 4 |
| Delete one thread | 2.71 s | 0.48 s | 110 B | 110 B | 19 → 2 |

These are single sequential trials on a busy host, not fleet throughput or p95
latency. Initial publication has more chunk-write overhead. Changed large threads
still require whole-thread replacement. The server still reads/decodes retained
history and computes complete canonical revisions and thread fingerprints, so
server CPU/read work remains proportional to node size. The catalogue and active
version filter also grow with thread count. Sustained backlog, peak memory and
full-size node latency need separate measurements. Private captures and the
executable harness are in
`/home/james/scratch/mash-artifacts/incremental-publication-20261004`.

## Full-node publication overhead

An isolated copy of the personal node contains 749 threads, 238,257 messages
and 32,606 embeddings. Replaying its unchanged retained gold took 31.72 seconds.
This sends the complete retained embedding stream and exercises server-side
thread reuse; it is not an ordinary watcher update or an inference benchmark.

Small canonical snapshots now use checksum-verified memory buffers: at most
1 MiB per part and 64 MiB in aggregate per receive operation. Larger parts and
overflow keep their existing disk spools. Decoding remains on blocking workers;
the stored representation, checksums and publication visibility do not change.
The first two unchanged replay trials with this receive path took 16.89 and
15.96 seconds, compared with the 31.72-second baseline.

Changing a single vector forced replacement of exactly one thread in each trial.
A small thread (87,997 canonical bytes) took 31.28 seconds before and 35.17 seconds
with memory receives. A large thread (54,590,067 bytes) took 53.85 and 73.85 seconds,
respectively. The latter included 46.2 seconds of optimizer drain. These results
do not establish an incremental latency improvement: backend waits and segment
layout varied on the busy host, and large updates still replace whole threads.

Shared HTTP responses negotiate gzip at the fastest compression level. Old
clients receive plain JSON; current shared clients decompress into the existing
typed, admitted decoder. Internal Qdrant reads stay uncompressed. Raw snapshot
buffers are transient rather than a cache retained between requests.

Five alternating real-network search pairs from the development MacBook returned
byte-identical decoded responses. Gzip reduced each response from 932,903 to
343,301 bytes (63% fewer bytes). Excluding the first warm-up pair, median candidate
request time was 1.295 seconds with plain JSON and 1.091 seconds with gzip. These
are HTTP candidate timings, not complete CLI/MCP search or a fleet p95.

An 8 MiB/1,024-point staging experiment reduced completed point-write requests
for the same large thread from 61 to 33, but acknowledged publication took
85.13 seconds versus 73.85 seconds with the existing 2 MiB/512-point limits.
Segment layout and host load varied. The larger limits were rejected because
this trial did not show a latency improvement; the existing limits are retained.

The Mac helper now uses a stable, checksum-synchronized source directory while
still running Cargo for every build. Its first validation run took 48.31 seconds
(32.33 seconds in Cargo); the identical-source repeat took 10.74 seconds
(0.42 seconds in Cargo), reusing the matching downloaded binary. This demonstrates
warm reuse, not a comparison against the old helper on different commits. Builds
sharing a remote root must be serialized.

Release `2a60e9d` was deployed to the shared API and all three nodes using one
validated artifact per platform, without reindexing. API readiness, unauthorized
rejection and publication identities passed the restart helper's checks. The
native feature-update build took 75.05 seconds, including transport and a
34.05-second Cargo build.

Actual `cbioportal` MCP checks after deployment returned successful thread
results without fallback:

| Node | Local-only | Warm local + shared | Shared candidates in warm combined call |
| --- | ---: | ---: | ---: |
| Xenia | 1.346 s | 2.144 s | 0.208 s |
| Development MacBook | 0.597 s | 2.625 s | 1.249 s |
| Personal MacBook | 1.077 s | 2.883 s | 1.277 s |

These are individual calls, not percentiles or a general latency guarantee.
The first post-deployment calls took 15.176 seconds on Xenia and 5.569/5.247
seconds on the MacBooks. Xenia's first-call delay is not explained by the recorded
retrieval/reranking stages, so this deployment does not establish a cold-search
fix. A later traced local call spent 458 ms loading models and 7.7 ms loading
vectors. Larger full-rebuild optimizer comparisons remain separate experiments.

Private corpus copies, requests, traces and validation logs are retained under
`/home/james/scratch/mash-artifacts/publication-throughput-20261005`.
