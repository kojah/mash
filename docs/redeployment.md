# Redeployment and index refresh

Build one Linux and one Apple Silicon artifact from the validated commit, then
reuse them across the nodes. The existing deployment workflow installs nodes
concurrently and atomically replaces each binary. It does not ingest history.
Existing MCP processes keep their running binary; reconnect clients to use the
installed release. Restart the shared API separately using its existing server
configuration and credentials, preserving Qdrant data.

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

## Incremental publication evaluation

The wire protocol already supports delta uploads with `snapshot=false` and a
`base_revision`; routed ingestion uses them. Qdrant publication still reconstructs
and encodes the complete node, stages all documents, conditionally publishes its
head and reclaims the previous version. Local publication triggers advance a
revision but do not retain a durable per-record change journal for this path.
Consequently, a small ordinary edit can still pay full-node publication costs.

The bounded-batch build also accepted delta additions of 1, 10 and 100 canonical
assistant messages on the copied corpus: 556/4,879/68,973 uploaded bytes and
2.78/3.17/2.99 s respectively. These context-only additions required no new
embeddings and retained all prior messages. The sample included one preparation
addition before this sequence. These are HTTP mutation times, excluding local
ingestion/preparation; existing snapshot staging still rewrote the whole node.
They are not measurements of arbitrary edits or deletion. The current protocol
rejects replacing an existing message ID directly in a delta: ordinary edits
use a thread replacement, another reason to define journal granularity explicitly.

An immutable segment/delta design must solve both sides of this cost. It needs a
durable change journal and a versioned server manifest, including explicit
deletions. Conversation metadata is duplicated into message payloads: title,
project or repository edits must invalidate dependent search documents too.
Retrieval must suppress superseded/deleted records before ranking, respect
filters and provide consistent thread/context lookup across active segments.
Privacy/model/source interpretation changes still require a full rebuild.

Evaluate bounded segments and compaction before committing to that design.
Compare initial publication, 1/10/100-message changes, deletion and restart
recovery; include complete preparation/upload/commit times, memory, filter
accuracy, concurrent reads and sustained watcher backlog. Preserve conditional
publication and acknowledged-write recovery. A delta POST alone is insufficient
evidence of an incremental storage improvement. This work is tracked in
`mash-mgd`; deployment/batching work is `mash-hia`.
