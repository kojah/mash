# Shared search after bulk publication

## Reproduction and scope

The October 5 investigation copied consistent Qdrant 1.19.1 collection snapshots
to owned, loopback-only services. It did not restart the live backend, evict its
cache, modify provider files or run new embedding inference. Private captures,
snapshot files and logs are outside Git at
`/home/james/scratch/mash-artifacts/cold-search-20261005`.

The copied message collection initially contained 1,018,215 physical points and
284,626 indexed vectors across eight segments. Those include obsolete versions;
they are not counts of active logical messages. The three active node publications
contributed 68,731, 32,606 and 42,150 embeddings. The search visibility filter
referenced 1,567 unique immutable thread/node versions.

The baseline workload used the actual locally generated, quantized `cbioportal`
query embedding, exact dense retrieval and the existing sparse/dense fusion. It replayed
an unfiltered `/v1/search` request through Mash's HTTP service. The replay omitted
the optional filter; ordinary CLI requests encode an empty `all` filter, which
produces an additional empty nested Qdrant condition. These have the same intended
semantics, but the replay is not byte-identical to the original failing MCP call.
Timings below cover shared candidate retrieval, not embedding inference, local
search, reranking or complete MCP latency.

An advisory cache eviction on the stopped, owned copy verified zero resident
pages in its storage files before restart. That cold-read trial returned dense
retrieval in 237 ms; cache coldness alone did not reproduce the timeout. Concurrent
invisible writes also produced slower reads without consistently exceeding the
deadline. They were a control, not a proposed production indexing strategy.

The decisive baseline used a real gold publication for the personal node:
749 threads, 238,257 messages and 32,606 retained embeddings. Uniformly scaling
the existing vectors by 0.999999 changed fingerprints and forced restaging without
new model inference, preserving cosine geometry up to floating-point rounding.
Both comparison copies also received 32,768 additional, invisible pressure points.
The normal staging, visibility commit and obsolete-version reclamation paths ran.

The baseline returned successful publication after 148.62 seconds. The immediately
following search hit its five-second deadline; its backend dense query eventually
finished in 5.31 seconds. The next API search completed in 258 ms. This reproduces
the reported slow-first-search behavior on copied real data.

## Validated publication readiness

The API now waits after publication/reclamation for green collection health,
a healthy optimizer, an empty optimizer queue and no running optimizer jobs.
Green health alone was insufficient in this experiment. Qdrant exposes the
queue and running jobs through its [optimization status API](https://api.qdrant.tech/api-reference/collections/get-optimizations).
Its [optimizer guidance](https://qdrant.tech/articles/tuning-qdrant-optimizer/)
describes the CPU and I/O competition during indexing.

Polling happens once per second and has a five-minute overall budget, including
stalled health reads. An already settled publication does not sleep. The active
gold configuration is checked again before success, since it can change during
the wait. Interactive search retains its existing five-second deadline; all
committed messages remain eligible for retrieval.

If optimization fails, pauses or exceeds the budget, publication returns an
error while preserving the already committed head and data. It does not replay
the mutation or roll back a confirmed commit. Tests cover these semantics,
private upstream-error suppression, stalled reads and configuration activation
during the drain.

An initial two-minute drain budget was too short: the full publication returned
HTTP 503 with its head committed and optimization still running. Searches issued
after that failure also timed out. Those observations are not successful fixed
publication measurements; they motivated the five-minute bound.

The subsequent drain-only trial acknowledged publication after 889.43 seconds
on the busy host, including approximately 500 seconds of document staging,
91 seconds of reclamation and 261 seconds of optimizer drain. Despite observing
an empty queue and no running jobs, the next three API requests timed out.
Their backend dense queries finished together: the oldest took 15.42 seconds,
while subsequent warm dense retrieval took roughly 70–90 ms. Optimizer drain
alone therefore did not fix this workload.

A node-only exact readiness probe was also insufficient. That trial used the
HNSW default and the ordinary empty `all` filter. Publication completed in
447.68 seconds, including a 134-second optimizer drain and a 2.35-second probe,
but the first shared query still timed out. The second took 4.26 seconds;
later queries took 277–443 ms.

The next revision probes all visible group versions, including other nodes.
It first performs exact retrieval with a synthetic unit vector and one result,
then exercises the configured HNSW path with a 200-result candidate budget.
Both probes omit payloads and use the existing 30-second backend-read budget
each. They require no user query or model inference. Probe failure preserves
the confirmed publication and prevents an indexing-success acknowledgement.
Focused tests cover the two retrieval modes, whole-group filter, sanitized
errors and publication preservation.

The whole-group revision completed the next full publication in 589.68 seconds,
including a 193.01-second drain and 13.52 seconds for both probes. The first
shared search returned HTTP 200 in 404 ms. All 30 consecutive searches returned
HTTP 200, ranging from 256 to 790 ms. These used the ordinary empty `all` filter
and the active HNSW policy with `ef_runtime = 200`. The published revision and
counts remained 819,027, 749 threads, 238,257 messages and 32,606 embeddings.

Approximate wall-clock breakdown from the publication trace:

| Stage | Seconds |
| --- | ---: |
| Receive upload, load state and prepare publication | 41 |
| Write retained snapshots and thread metadata | 82 |
| Write messages, dense vectors and lexical data | 210 |
| Commit publication and reclaim obsolete versions | 49 |
| Drain optimizer jobs | 193 |
| Exact and HNSW read probes | 14 |
| Final checks and response | <1 |
| Total | 590 |

The upload itself took approximately two seconds and is included in the first
row. There was no embedding inference. This deliberately forced full replacement
is not a representative small incremental update.

This meets the reproduction's acceptance check: successful publication followed
by 30 searches within the existing five-second deadline. It supports the combined
drain and whole-group read-probe mitigation; it does not isolate each component's
effect. Arrival order, segment layout and other host workloads changed between
trials, and the original baseline used exact retrieval. The preceding node-only
trial also used HNSW and still failed, so the default change alone was insufficient.

After this publication, exact and HNSW retrieval returned the same 200 semantic
candidate identities for the query in three scopes: unfiltered, Codex-provider
filtered and personal-node filtered. Identity comparisons include node, thread
and message ID; they do not compare storage point IDs. This is a narrow
retrieval-consistency check, not a general recall or relevance evaluation.

Shared search defaults to filtered HNSW with `ef_runtime = 200`. Exact retrieval
remains an explicit query/configuration option and the comparison reference.
The existing fleet catalogue requires explicit query activation to switch from
its persisted exact policy; changing a Rust default alone does not deploy that
change. Activation preserves silver, build identities and published node data.

## Limits

This is a contention mitigation tied to indexing completion, not a latency
guarantee. Searches while other writes or optimizer work are running can still
contend, and continuous concurrent writers can exhaust the bounded drain. A
restart or unrelated physical cache state can also affect latency. The benchmark
uses one query and a busy Linux host; it does not establish fleet p95 latency,
HNSW performance or search-quality gains. Repeated publications change segment
layout, and the small vector scaling permits numerical/tie differences.
