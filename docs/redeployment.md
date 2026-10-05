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

## Paired optimizer-parallelism screen

Two copies of the same stopped, healthy backend started with 1,054,824 message
points, 362,239 indexed vectors and seven segments. Both used release `2a60e9d`,
the same geometry-preserving vector-change input, existing 2 MiB/512-point write
batches and the bounded snapshot receive path. There was no model inference.
The two-job trial ran first; the default trial followed on the same busy host.

The experiment set `max_optimization_threads = 2` on both collections in one copy,
leaving the other copy at Qdrant's automatic default. This controls concurrent
optimizer jobs per shard, rather than the worker threads within an individual
job; see [Qdrant's configuration reference](https://qdrant.tech/documentation/ops-configuration/configuration/).
Progress samples observed at most two running message-collection jobs in the
capped trial and five at the default.

| Measurement | Automatic default | Two-job cap |
| --- | ---: | ---: |
| Acknowledged full gold publication | 427.28 s | 360.47 s |
| Stage snapshots, catalogue and documents (inclusive) | 145.22 s | 215.71 s |
| Reclaim obsolete versions | 4.90 s | 53.10 s |
| Drain optimizer work | 259.00 s | 68.31 s |
| Exact and HNSW read probes | 1.80 s | 6.48 s |
| First following shared candidate search | 357 ms | 414 ms |
| Mean of 30 following searches | 320 ms | 306 ms |

Both publications retained 749 threads, 238,257 messages and 32,606 embeddings.
All 60 following searches returned HTTP 200 within the existing five-second
deadline. The first queries returned the same 200 semantic candidate identities
(node, thread and message), despite different storage versions. This is one
query's consistency check, not a general recall evaluation.

The cap improved acknowledged publication by 15.6% in this paired screen while
slowing staging and reclamation. It was applied to this Xenia server's existing
message and state collections, with publication identities verified unchanged.
The setting also survived restart of the owned test backend. HNSW parameters,
completed-write acknowledgements and committed-message visibility are unchanged.
This is a host-specific operator setting, not a new application-wide default.
The setting is persisted by Qdrant; new groups need their own operator policy.

Starting storage and input were matched, but timing, cache state, segment evolution
and unrelated workloads were not controlled or repeated. The result supports this
bounded host tuning; it does not establish a fleet-wide ingestion speedup, initial
embedding throughput, a latency SLA or a percentage improvement over the earlier
590-second run. The paired source copies, progress samples, traces and responses
remain private under
`/home/james/scratch/mash-artifacts/optimizer-parallelism-20261005`.

The Mac helper's next, documentation-only commit (`919237b`) completed in
5.69 seconds, with Cargo taking 0.18 seconds and the executable checksum unchanged.
This verifies reuse across commits with unchanged Rust inputs, not just repeats
of the same revision.

Private corpus copies, requests, traces and validation logs are retained under
`/home/james/scratch/mash-artifacts/publication-throughput-20261005`.

## Disconnected deployment targets

The build and remote-install helpers disable SSH control-socket reuse for their
own connections, use a ten-second connection timeout, and set keepalives to
15 seconds with two unanswered probes permitted. Reusing an existing master
would otherwise retain its older keepalive policy and could hang after a node
disconnects. These settings bound an unresponsive transport, not a healthy
long-running remote build.

With fresh connections, the cached development MacBook build completed in
9.46 seconds (Cargo 0.15 seconds), and the identical-artifact install check in
1.82 seconds. The unavailable personal MacBook failed after 20.07 seconds:
the digest check and subsequent installation connection each hit the
ten-second timeout. Its new binary had already been deployed and had passed
search checks earlier; a final repeat is pending connectivity.

After the optimizer setting changed, warm combined MCP search took 1.473 seconds
on Xenia and 2.991 seconds on the development MacBook without fallback. An
earlier development MacBook call hit the five-second shared deadline and fell
back to local results. Healthy optimizer status alone therefore does not prove
a latency guarantee. Cold-search outliers and a broader latency distribution
remain tracked in `mash-16b`.

## Local readiness index

Schema 43 adds a partial index on message role for nonempty interpreted dialogue
(`COALESCE(search_projection, content) <> ''`). Readiness still checks the same
eligible-message count against packed vectors; it no longer needs to scan message
bodies. Combined retrieval checks readiness during preparation and again when
opening local candidates, so this removes repeated canonical-text reads.

The additive migration preserves canonical history, embeddings, gold manifests,
readiness markers and shared publication identity. As with other schema upgrades,
run an explicit `index --rebuild-local-gold` before validating the upgraded node.
This reads stored silver offline; it does not rescan providers or publish shared
gold. The private Xenia-copy upgrade took 0.23 seconds with zero model inferences,
zero changed messages and zero removed messages.

On that frozen 1.1 GiB copy, the index occupied 720,896 bytes (704 KiB). Ten count
queries per selection retained exactly 42,150 eligible messages and 7,668 eligible
user messages. Table scans took approximately 100–110 ms; indexed counts took
approximately 1–2 ms. SQLite's plans confirmed use of the partial index for both
selections. The migration test also covers projection edits and deletion.

Eight fresh-process local searches before migration and eight afterward returned
identical ordered thread and message identities. Median core search time was
1.891 seconds before and 1.626 seconds afterward. These sequential runs overlapped
compilation on a busy host; they are a screen rather than a controlled percentage
speedup or a cold-disk latency guarantee. Model loading and contextual reranking
remain part of complete search latency.

Separately, twelve fresh Xenia combined CLI searches took 1.636–2.065 seconds,
and twelve fresh development MacBook combined MCP searches took 2.292–2.595 seconds,
all without shared fallback. These repeated one query on warm filesystem/backend
caches, not a diverse-query or sustained-arrival benchmark. Private snapshots,
plans, identities, traces and test logs are retained under
`/home/james/scratch/mash-artifacts/search-tail-20261005`.

Release `2366e2f` was installed on Xenia and the development MacBook. Their explicit
stored-silver upgrades took 315 ms and 1,851 ms in the indexing core, respectively,
with zero inference and zero changed/removed messages. Local and shared status
checks succeeded. Fresh combined MCP searches returned three threads without
fallback in 1.713 seconds and 2.255 seconds, respectively. The native Mac build
took 52.30 seconds (Cargo 31.21 seconds). The personal MacBook remains unreachable;
its prior deployment is retained and this additive upgrade is pending. The shared
API and Qdrant were not restarted for this local-only schema change. Existing MCP
clients need their normal reconnect after binary replacement.

## Incremental publication under concurrent reads

A stopped copy of the shared corpus ran on an isolated Qdrant 1.19.1 backend,
with the existing two-job optimizer cap, HNSW ef 200 and unchanged write batches.
The personal-node copy retained 749 threads, 238,257 messages and 32,606 embeddings.
A metadata-only edit uploaded one complete 29-message thread with five retained
vectors, approximately 90 KiB, through the normal pinned `/v1/index` delta protocol.
There was no inference, provider scan or local SQLite ingestion in these trials.

The initial delta took 21.797 seconds. Eight title updates arriving two seconds
apart were then coalesced by a serial test controller into two publications taking
15.680 and 16.832 seconds. The final title was verified through thread listing;
all nodes retained their thread/message counts. The last arrival was acknowledged
18.684 seconds later, with the burst drained in 32.738 seconds. The native watcher
already bounds pending notifications to one pass; this controller exercises the
same latest-state coalescing pattern but does not measure native watcher latency.

During the burst, 39 of 40 direct candidate searches returned HTTP 200 and one
returned a retryable HTTP 409 because publication changed the group catalogue
during its read. Across all responses, median latency was 335 ms, nearest-rank
p95 415 ms and maximum 823 ms. The client now retries that read-only search
conflict once after 25 ms within the original five-second admission/request/body
deadline. Indexing, gold publication and other mutations are never replayed;
authorization failures and service-unavailable search responses are not retried.
A persistent conflict reports a search retry action rather than indexing advice.

A second burst used the actual updated CLI: all 20 fresh-process shared-only
searches returned three threads without fallback, in 1.549–2.471 seconds. Its
publications took 17.713 and 17.088 seconds. No conflict occurred in those client
traces, so deterministic transport tests, rather than this timing run, establish
the retry behavior and its two-attempt limit.

The first direct search in the initial trial timed out after eight seconds while
overlapping 6.14 seconds of obsolete-version cleanup. Five reads after an owned
backend restart returned HTTP 200 in 279–393 ms on warm filesystem caches. However,
a standalone actual shared-only CLI call before the second burst hit its
five-second request deadline. These observations do not isolate cold-cache effects
from backend maintenance or establish a cold-read guarantee. Startup warming is
not justified by this screen alone. These remain explicit follow-ups in `mash-16b`.

This was one query on one busy host, with retained real history and edited thread
metadata. It is not an inference, semantic-quality, production watcher or all-node
latency benchmark. Requests, pinned heads, traces, arrivals, acknowledgements and
verification results remain private under
`/home/james/scratch/mash-artifacts/incremental-backlog-20261005`.

Release `bf2c847` was installed on Xenia and the development MacBook without
reindexing. Local/shared status calls succeeded and fresh combined MCP calls
returned three threads without fallback: Xenia initially 4.151 seconds and then
1.771 seconds; development MacBook 4.661 seconds. The native build took 53.26
seconds (Cargo 31.53 seconds). The spread remains relevant to cold-read work;
these deployment checks are not a latency SLA. The personal MacBook is confirmed
offline by Tailscale, with its last-seen timestamp at 06:30 UTC, so its upgrade
and final checks remain pending. The owned benchmark API/backend were stopped;
the live shared API and Qdrant were unchanged. Validation passed 602 standard
tests, four retry-boundary tests, strict Clippy, formatting and doctests.

### Native-request attribution under disk contention

Matching the earlier failed CLI call to its server trace shows 4.946 seconds in
`backend_search`, followed by roughly 112 ms before HTTP 200 completed. The client
expired at its five-second shared-request deadline. Admission was immediate and
query preparation took 523 ms outside that deadline. This isolates the timeout to
backend retrieval and response handling, but does not identify the slow native
Qdrant request in that earlier run.

A subsequent private diagnostic proxy recorded individual Qdrant requests against
the same retained corpus. The owned backend needed approximately 110 seconds to
restart while host I/O pressure was high: kernel I/O `some` stalls averaged 40%
over a sampled minute. This is a contention stress screen, not a normal startup
estimate. No live service was restarted and no global cache was dropped.

Six fresh-process shared-only searches succeeded in 1.934–2.753 seconds;
candidate retrieval took 317–411 ms. Four searches exhausted the existing
one-second context budget and reported incomplete shared context. The third
returned thread varied in one search. Therefore, successful responses do not
establish identical ranking or complete reranking evidence under this load.

The proxy recorded 12 vector queries, 222 state-point reads, 398 message scrolls
and 98 message-point reads, including initialization and context requests that
continued after cancellation. These are not all candidate-search requests:
neighbor retrieval for reranking accounts for most of the fan-out. The slowest
native request observed was 202 ms; replaying the captured dense query directly
without the proxy took 147–195 ms across six reads. It used HNSW ef 200,
`exact=false`, and a 200-candidate limit. The proxy buffers responses and adds a
connection hop, so these measurements are diagnostic rather than a production
latency comparison.

The next bounded opportunity is batching neighboring-message retrieval while
preserving node isolation, topology selection, publication consistency,
redaction and the existing cancellation budget. Increasing deadlines or reducing
reranking evidence would obscure the issue rather than establish equivalent
search quality. This screen does not justify either change or startup warming.
Private captures are in the `native-timing` subdirectory of the corpus benchmark
above; they are not included in Git.

### Rejected parallel-neighborhood prototype

A smaller preliminary change fetched the anchor outline, ancestor chain and
descendant chain concurrently, retaining order within each chain and the same
union validation and topology selector. A barrier-based transport test required
all three independent paths to start together and compared the selected IDs
against the full topology reference. Three existing Qdrant context contracts,
603 standard tests, strict Clippy, formatting and doctests passed. Two initial
disk-backed integration fixtures timed out creating payload indexes before
exercising the change; all three context contracts passed on a separate disposable
Qdrant instance with storage in tmpfs. That verifies context behavior, not disk
durability or performance.

The old and prototype APIs then read the same retained, disk-backed corpus using
the same unchanged CLI. After separate warm-up queries, four ABBA rounds gave
eight measurements per API. Median end-to-end search was 1,737 ms before and
1,798.5 ms after; median context retrieval was 126,043 and 136,719 microseconds.
All 16 measured searches had complete context, no shared fallback, and identical
ordered thread and message identities. The host remained busy and compilation
overlapped early measurements, so this is a rejection screen rather than a
precise regression estimate. It provides no evidence of a useful speedup. The
prototype was reverted and was not deployed. Its patch and captures remain
private under `/home/james/scratch/mash-artifacts/parallel-context-20261005`.

The first baseline warm-up attempt also reproduced the cold candidate timeout.
Qdrant's own request log recorded the dense query at 5.397 seconds and the parallel
lexical query at 0.145 seconds. The client expired at its unchanged five-second
deadline before context retrieval began. This attributes that outlier to native
dense retrieval, independently of the rejected context prototype.

Qdrant's [memory-tier documentation](https://qdrant.tech/documentation/ops-configuration/memory-tiers/)
states that cached vectors and graph data can be evicted from the OS page cache;
cached placement does not guarantee residency. Pinning graph links is a possible
separate screen, but dense vectors cannot use the pinned tier. No placement,
deadline, ranking evidence or live-service setting changed in this experiment.
The remaining batched-context task is `mash-hu2`; cold dense-read diagnosis remains
part of `mash-16b`.

### Graph-residency screen

On the owned retained copy, the first dense query after another graceful restart
took 159 ms when host I/O pressure was low; a following query took 181 ms.
The actual collection used cached dense vectors and cached HNSW links. Exact
counts found 1,018,215 points matching the captured active-publication filter
among 1,054,824 live points (96.5%). This does not support obsolete staging as
the dominant explanation for the earlier slow query.

Changing only the private message collection's HNSW memory tier to pinned
triggered an optimizer conversion. The first sampled green status occurred
approximately 193 seconds after the PATCH. The Qdrant process's observed
VmHWM reached 9,303,240 KiB during conversion; that includes mapped/file-backed
pages and startup, and is not an estimate of graph heap size. The settled query
took 254 ms, and the first query after a subsequent graceful restart took 241 ms.
Its top-200 candidate set and per-ID scores matched the cached baseline exactly;
some tie ordering changed. Four following native queries without payload took
126–156 ms, while interleaved full-payload queries took 146–200 ms.

The post-restart Qdrant process gained 13,162 minor faults and no major faults
during the first measured query. Initial counter captures accidentally targeted
the container's shell entry point; those were explicitly discarded. There was
no global cache drop, private-file eviction, matched high-pressure comparison,
or end-to-end inference benchmark. The low-pressure cached and pinned samples
therefore do not establish a precise regression or prove pinning fixes cold
outliers. They provide no evidence to promote this setting.

The owned backend was stopped and production was unchanged. Captures remain
private under `/home/james/scratch/mash-artifacts/graph-residency-20261005`.
The diagnostic copy intentionally retains pinned graph placement, which the
current Mash API's cached/cold schema rejects. Restore cached placement and wait
for optimizer completion before reusing that copy for API benchmarks.

### Candidate catalogue read simplification

Release `364d848` reads one validated active catalogue for both node visibility
and vector policy. Previously, a second read selected the policy, and an eager
`unwrap_or` evaluated that read even with an explicit request override. The
post-query catalogue check still rejects changed heads; recipe, generation,
filter and payload validation are unchanged. HNSW ef 200 remains the default.

The shared API was deployed without indexing, with authenticated readiness and
publication identities preserved. Validation passed 602 standard tests, five
real-backend query-policy/activation contracts, strict Clippy, formatting and
doctests. The optimized release build took 58.77 seconds. This is a server-only
change and does not require replacing node binaries or reconnecting stdio MCPs.

Eight pre-deployment candidate calls all succeeded: the first took 3.101 seconds
and the others 137–192 ms. Eight post-deployment calls took 171–209 ms while fresh
MCP verification ran concurrently. All saved pre/post response bodies matched
after canonical JSON serialization. The overlapping workload prevents treating
those timings as a direct speed comparison.

A subsequent alternating ABBA comparison used old and new APIs reading the same
live backend: all 32 requests succeeded, 16 per variant, with medians of 170.054
and 167.164 ms respectively. The 16 retained paired response bodies matched;
repeated filenames overwrote the first body in each same-variant pair. The small
timing difference on this busy host is not a significant fleet speedup or a fix
for cold dense reads. The benefit established by the code change is one fewer
catalogue request and one coherent snapshot for policy and visibility.

Fresh combined MCP searches returned three threads, complete shared context and
no fallback in 1.845 seconds on Xenia and 2.259 seconds on the development MacBook.
The personal MacBook remains offline and its outstanding upgrade/checks remain
pending. The temporary baseline API and disposable correctness backend were
stopped; production continues on the new API. Private evidence is in
`/home/james/scratch/mash-artifacts/catalogue-read-20261005`.

### Native local refresh and pending-queue lookup (2026-10-05)

A consistent private copy of Xenia's local database was refreshed against its
real provider files with installed local models, using local-only publication.
The installed database and shared index were untouched. The catch-up pass took
65.21 seconds: ingestion 58.914 seconds, search refresh 298 ms, and the embedding
phase 4.293 seconds (including model setup and storage work). Stored messages
grew from 345,451 to 353,102; 1,162 vectors were added, with 1,007 model inferences
and 155 reused embeddings. Peak RSS was 1,211,624 KiB. This is a backlog refresh,
not initial indexing from an empty database.

The next incremental pass took 14.63 seconds for 11 changed messages and two
inferences. It skipped 821 of 823 sources by checkpoint; ingestion took 7.874
seconds and the embedding phase 4.265 seconds, including 635 ms model loading.
Peak RSS was 370,316 KiB. These different workloads are not a before/after speed
comparison, and the embedding phase is not pure model inference time.

The user-only embedding-queue restriction previously enumerated every non-user
message before deleting matching queue entries, twice when rebuilding pending
embeddings. It now iterates queued IDs and checks each message's role through its
primary-key index. Rollback-only ABBA trials on the 353,113-message copy preserved
identical removal counts. With 12 queued messages, each old call took 622–665 ms
versus 0.021–0.191 ms for the new query. With all messages queued, old calls took
745–853 ms versus 936–1,007 ms for the new query; both removed 310,782 entries
and retained 42,331 user entries. This favors frequent small incremental updates
at a modest full-queue cost. It requires no schema migration or reindexing.
This query runs only with `dense_content = "user-messages"`. The deployed fleet
currently uses the default `all-visible` recipe, so this optimization does not
reduce its refresh time. The different native refresh timings below must not be
attributed to this query change.
The isolated timings do not establish a complete ingestion speedup, watcher
latency, or MacBook performance. Private evidence is in
`/home/james/scratch/mash-artifacts/native-refresh-20261005`.

A following native refresh using the optimized release completed in 6.85 seconds
for 119 changed messages and 13 inferences, with 821 sources skipped. Ingestion
took 4.735 seconds; the embedding phase took 1.005 seconds including 687 ms model
loading. Peak RSS was 394,812 KiB. Source changes and host scheduling differed
from the earlier pass, so this is a successful production-data smoke test rather
than a controlled before/after speedup. The focused role-selection regression,
603 standard tests, strict Clippy, formatting and doctests passed. Xenia's binary
was installed without indexing; local status remained ready/hybrid. A fresh
combined MCP search returned three threads in 1.792 seconds, with complete shared
context and no fallback.

The same source was built natively for ARM64 on the development MacBook (Cargo
31.72 seconds), installed atomically, and checked without indexing. Local and
shared status succeeded; a fresh combined MCP search returned three threads in
2.798 seconds with complete shared context and no fallback. The personal MacBook
remains offline (last seen 06:30 UTC), so its outstanding deployment remains
pending. Existing stdio MCP sessions require the usual client reconnect to use
the replacement binary; the live shared API and Qdrant were not restarted.

### Snapshot serialization buffering (2026-10-05)

The full-node incremental trace shows 750 snapshot encodes per publication
(metadata plus 749 threads), totaling 3.57–4.32 seconds of encoder busy time.
The encoder buffered file writes but fed each small JSON token directly to
BLAKE3. Its existing 64 KiB buffer now precedes both checksum updates and file
writes. Canonical bytes, chunk boundaries, digests, admission limits and retained
history validation remain unchanged; this adds no cache or snapshot-sized buffer.

Two alternating ABBA cycles compared retained old/new optimized test executables
using the existing deterministic 10,000-message synthetic fixture. Every run
encoded 40,046,007 bytes. Old encoding times were 279, 539, 274 and 263 ms;
buffered times were 172, 180, 182 and 173 ms. Medians were 276.5 and 176.5 ms,
respectively, about 36% lower. The unchanged canonical-hash control took 21–25 ms.
This busy-host microbenchmark does not establish a complete publication speedup,
MacBook performance, or cold-search improvement.

Regression checks compare complete bytes and checksums with direct Serde
serialization and enforce exact limits around 64/128 KiB buffer boundaries,
including errors during the final flush. Existing Unicode chunk-boundary tests
also apply. The real-Qdrant incremental reuse/edit/delete contract passed on an
owned disposable tmpfs backend. Private evidence is in
`/home/james/scratch/mash-artifacts/snapshot-buffering-20261005`.

Release `b635dbc` passed 604 standard tests, strict Clippy, formatting and
doctests, plus the real-backend contract above. The release build took 46.09
seconds. It was deployed only to the shared API: authenticated readiness and
unauthorized rejection passed, and publication identities were preserved. No
indexing or Qdrant restart was performed. Fresh combined MCP searches returned
three threads with complete shared context and no fallback in 1.527 seconds on
Xenia and 2.442 seconds on the development MacBook. These are search smoke checks,
not effects of this publication-only optimization. Node binary replacement is
unnecessary for this change; the personal MacBook's earlier upgrade remains
pending connectivity.

### Complete incremental publication with buffered encoding (2026-10-05)

One warm ABBA cycle compared pre-buffering API `364d848` with `b635dbc` on the
same private retained-corpus backend. Before the comparison, its earlier pinned
graph experiment was restored to cached placement; the healthy optimizer reached
green after 236 seconds of sampled conversion time. No timed publication ran
during that conversion. This restoration changes segment layout, so timings
cannot be compared directly with the older pinned experiment.

Each run made a small title edit to the same complete 29-message thread, using
the same retained vectors, without model inference. An initial old-API warmup
took 20.160 seconds and was excluded. Timed HTTP publications took 11.221 and
12.372 seconds with the old encoder versus 9.484 and 9.354 seconds with the new
encoder. Medians were 11.797 and 9.419 seconds, approximately 20% lower.

For each timed request, all 750 snapshots were still decoded and encoded:
encoder busy time fell from 3.103–3.707 seconds to 1.100–1.138 seconds, while
decoder busy time remained 2.798–3.064 seconds. All four responses were HTTP 200
and retained 749 conversations, 238,257 messages and 32,606 embeddings. Freshness
confirmed current lexical/gold manifests and ready metadata, lexical and dense
artifacts after every publication. Request times include upload, retained-state
reads, planning, serialization, staging and acknowledgement; phase spans are
instrumented work, not a complete partition of wall time.

This single alternating cycle is evidence of a complete warm small-edit
publication improvement on real retained history, not initial-ingestion
throughput, p95 latency, sustained watcher backlog, MacBook inference performance
or cold-search performance. Temporary APIs and the owned backend were stopped;
the production API's readiness and authorization checks still passed without any
production indexing or Qdrant restart. Private captures and the executable
comparison are under
`/home/james/scratch/mash-artifacts/publication-buffering-20261005`.

### Cold filtered local search (2026-10-05)

A 120-request MCP matrix across Xenia and the development MacBook exercised
local, shared and combined scopes, including provider/time, role, project,
thread, exact-mention and no-match filters. All requests succeeded without
shared fallback or incomplete context. The first provider/time local search
spent 3.552 seconds in dense retrieval on Xenia and 1.581 seconds on the MacBook.
The local eligibility query scanned canonical message rows, including messages
that have no searchable projection.

Schema 44 adds a partial metadata index over searchable message timestamps,
conversation IDs and roles. Filtered dense retrieval explicitly uses this index
to avoid scattered canonical-body reads; the smaller role index remains for
readiness counts. Migrations from schemas 42 and 43 preserve readiness and
publication identity without replaying sources or recomputing embeddings.
The index occupied 2,666,496 bytes on the private 353,232-message database.

One ABBA comparison used separate identical private SQLite copies, fresh MCP
processes and file-specific eviction advice before each run. An unfiltered
primer loaded models and packed vectors, followed by the same Codex/time-filtered
query. Old complete query times were 3.846 and 3.510 seconds; new times were
0.483 and 0.373 seconds. Dense retrieval took 3.614/3.187 seconds before and
0.229/0.107 seconds after. All four complete result arrays were identical.
This isolates cold filtering after model loading; it does not establish a
fully cold CLI latency, fleet p95, or initial-indexing throughput improvement.
Production files received no cache-eviction advice.

A preliminary change that only added the searchable predicate was rejected:
it selected the smaller role index and caused scattered message-table reads,
increasing dense retrieval from 3.0–3.2 to 5.2–5.5 seconds. The final metadata
index avoids those reads. Private captures, including the rejected comparison,
are under `/home/james/scratch/mash-artifacts/query-matrix-20261005`.

Release `30f288b` passed 605 standard tests, the final focused migration/filter
regressions, strict Clippy, formatting and doctests. A further ten-case private
old/new MCP comparison preserved complete result arrays in every case. The
Linux release took 45.84 seconds to build; the native ARM64 build took 30.92
seconds. Both reachable nodes were installed atomically, then explicitly migrated
using stored-silver local gold maintenance. Xenia took 176 ms and the development
MacBook 1,010 ms; both scanned zero provider files and ran zero model inferences.
Local/shared status remained hybrid and ready.

After deployment, another 60 real MCP requests covered ten query types and all
three scopes across both nodes. Every request succeeded without shared fallback
or incomplete context. The provider/time local query took 264 ms on Xenia and
24 ms on the development MacBook (which returned no local matches); these warm
smoke timings are not a matched fleet speedup or p95 estimate. The shared API and
Qdrant were not restarted or reindexed. The personal MacBook remains offline
(last seen 06:30 UTC), with its ARM64 artifact staged for the same explicit
migration when connectivity returns. Existing stdio sessions still require a
client reconnect to load the replacement binary.

### Bounded decoding of retained file snapshots (2026-10-05)

With the buffered encoder deployed, another private retained-corpus burst sent
eight title edits two seconds apart. The serial controller coalesced them into
two successful publications taking 19.534 and 10.593 seconds. It drained in
30.298 seconds; the final arrival was acknowledged 16.264 seconds later. Of 40
concurrent candidate reads, 39 returned HTTP 200 and one a retryable catalogue
change conflict. Median read latency was 234 ms, nearest-rank p95 262 ms and
maximum 300 ms. This tests latest-state coalescing on the HTTP publication path,
not native watcher or local inference throughput.

File-spooled snapshots up to 32 MiB now use a temporary bounded buffer and Serde's
slice decoder; larger snapshots keep the streaming reader. Only one such buffer
exists per decoder at a time, independently of node size. Existing aggregate
receive and mutation admission remain unchanged; they are serialized-work
budgets, not process RSS limits. Typed models, checksum verification and generic
non-sensitive errors remain intact, with short/growing spool rejection tests.

A first private ABBA comparison overlapped validation and substantial host I/O
pressure. New publications took 27.879/39.631 seconds versus 11.051/14.821 seconds
before, with most added time waiting on Qdrant staging, pointer writes or cleanup.
Those runs remain in the record and were not used as speedup evidence. After
validation finished, another ABBA cycle on the same retained corpus took
9.864/10.452 seconds before versus 9.236/9.145 seconds after, approximately 10%
lower at the median. All responses retained 749 threads, 238,257 messages and
32,606 embeddings. Across 750 snapshots per timed request, decoder busy time fell
from 2.963/3.230 seconds to 2.103/1.947 seconds; encoder busy time remained
1.151–1.186 seconds. Per-process VmHWM samples were 869,752–926,840 KiB; these do not
establish an RSS regression bound or a fleet memory estimate.

The change passed 606 standard tests, strict Clippy, formatting and doctests,
plus the real-Qdrant incremental reuse/edit/delete contract on a separate owned
tmpfs backend. Measurements exclude inference, provider scans and local SQLite
work, and do not establish initial-ingestion throughput or p95 publication
latency. Private captures are in `watcher-burst-20261005` and
`decode-spool-20261005` under `/home/james/scratch/mash-artifacts`.

Release `aff5c9b` was deployed only to the shared API. Authenticated readiness,
unauthorized rejection and preservation of publication identities passed.
Fresh combined MCP searches returned three threads with complete shared context
and no fallback in 1.189 seconds on Xenia and 2.078 seconds on the development
MacBook. Those are smoke checks, not measured decoder effects on search latency.
Node binaries and indexes were not changed; Qdrant was not restarted. Both owned
benchmark backends and all temporary APIs were stopped after validation.

### Covered local embedding readiness (2026-10-05)

The native Linux watcher was exercised against a private full-corpus database
with three cloned real Codex sessions and retained attachment blobs. Twelve
isolated edits completed real local inference with median visibility latency
2.595 seconds and maximum 3.086 seconds, including the default one-second
debounce. Median indexing core time was 1.591 seconds; model loading took 502 ms.
Twelve additional edits arriving 200 ms apart coalesced into one completed pass
and drained in 5.191 seconds, with the final arrival acknowledged after 2.777
seconds. Every edit remained certified/hybrid and the final exact search passed.
This is local native watcher evidence, not shared publication or MacBook timing.

Readiness checked eligible IDs through message bodies and separately checked for
surplus vectors. Schema 45 adds a partial `(id, role)` index (1,843,200 bytes on
the private corpus). Equal cardinality plus inclusion is exact because message
IDs are unique: every eligible message must have a vector in the requested
generation, and no surplus current-generation vectors can remain. Missing,
wrong-generation, hidden and equal-count wrong-member regressions pass. Additive
migrations from 42–44 preserve publication/readiness and track projection edits.

A first eight-pass paired comparison encountered substantial host I/O pressure
and mixed results; old core times were 23.425/3.272/3.014/4.704 seconds versus
15.133/12.402/4.437/4.091 seconds after. These remain in the record and are not
speedup evidence. A following paired comparison explicitly read each owned
private database before timing, with no global cache manipulation. Old core
times were 2.273/2.687/2.068/2.122 seconds; new times were
2.080/1.748/2.043/1.527 seconds. Medians were 2.198 and 1.896 seconds, about 14%
lower. Each pair indexed the same one-message edit with one inference, identical
gold manifests and identical new vector bytes. This is a warm indexing result,
not a cold-cache, fleet p95 or sustained shared-watcher guarantee.

The change passed 607 standard tests, strict Clippy, formatting and doctests.
Private inputs, setup failures, native watcher output and paired timings remain
in `native-watcher-20261005` and `readiness-coverage-20261005` under
`/home/james/scratch/mash-artifacts`.

Release `407634c` was installed on Xenia and the development MacBook after the
Linux release build (46.26 seconds) and native ARM64 build (31.04 seconds).
Explicit stored-silver migrations took 178 and 867 ms, respectively, with zero
provider scans and zero model inferences. Both retained complete silver and
hybrid readiness. Fresh combined MCP searches returned three threads with
complete shared context and no fallback in 2.818 and 2.850 seconds. These are
deployment smoke checks rather than search speedup evidence. The shared API and
Qdrant were not restarted or reindexed. The personal MacBook remains offline;
the ARM64 artifact is staged for its explicit upgrade when reachable. Existing
stdio clients need their normal reconnect after binary replacement.

### Watcher-scoped embedding model reuse (2026-10-05)

Each running watcher retains at most one idle local indexing pool. CLI and MCP
watching share this scope through the application core, including its blocking
workers. Ordinary one-shot indexing retains no pool. Every checkout validates
the installed assets and matches the asset directory, marker timestamp, and
inventory sizes/timestamps. Changed assets cannot reuse the previous pool;
missing local assets or switching to remote inference clears it. Remote clients
are never retained. The pool keeps both asset-generation and lazy Core ML cache
leases, and normal watcher shutdown releases the resources on a blocking worker.
This does not change embeddings, inference batching, indexing readiness or the
shared publication protocol.

The development MacBook comparison used the same reset private SQLite baseline
(435,167 messages), retained attachment blobs, three copied real Codex sessions,
installed Core ML models, fixed probe timestamps and Low Power Mode off. Each
binary received twelve isolated additions followed by twelve arrivals 200 ms
apart. The order was old then new, rather than randomized.

| Measurement | Previous loading | Watcher reuse |
| --- | ---: | ---: |
| Median arrival-to-ack, including one-second debounce | 5.175 s | 1.947 s |
| Median indexing core time | 4.155 s | 0.927 s |
| Median model-load time | 3.213 s | 0 ms |
| First isolated arrival-to-ack | 5.397 s | 18.193 s |
| Complete twelve-arrival burst drain | 7.378 s | 4.267 s |
| Final burst arrival-to-ack | 5.076 s | 1.978 s |
| Sampled watcher peak RSS | 1,101 MiB | 1,229 MiB |
| Sampled median RSS between isolated passes | 931 MiB | 1,127 MiB |

The new first update still loaded its model (16.104 seconds); the following
eleven loads and the burst reused it. A preceding setup load with the old binary
also took 16.052 seconds. Preserve these outliers: the experiment establishes
lower steady update costs, not improved cold loading. Both watchers exited
cleanly, every addition retained complete/hybrid readiness, and all 24 persisted
probe IDs, generations, vector bytes and norms matched exactly. The new median
visibility latency was about 62% lower on this sample.

Memory samples used `ps` every 100 ms for the watcher PID only. Sampled peaks are
lower bounds, exclude device-wide/Core ML memory, and have unmeasured monitoring
overhead. Retaining models increases idle memory while watching is active. These
are local-only measurements, not initial-ingestion throughput, fleet p95,
sustained shared-publication backlog or a guarantee for the offline personal
MacBook. Private scripts, setup failures, vector fingerprints and raw captures
are in `/home/james/scratch/mash-artifacts/watcher-model-reuse-20261005`.

Validation passed 612 standard tests, strict Clippy, formatting, doctests,
the real-model Linux reuse/vector-parity test, all six indexing-cache tests
on native ARM64 (including real Core ML parity), and the real Qdrant scoped
message-search contract on a disposable backend. The backend was stopped after
its test completed; no live shared API or Qdrant index was changed.

A private native stdio MCP lifecycle check also completed three real-inference
updates: first load 3,496 ms, second load 0 ms, then 3,187 ms after stopping and
restarting watching. Every update retained complete/hybrid readiness and the
MCP exited cleanly. This verifies shared-core reuse and stop/restart lifecycle;
its repeated SQLite visibility polling is not a clean MCP latency benchmark.
