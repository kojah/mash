# Physical gold index construction and replacement

Status: implementation contract for `mash-q3e.3.7`, not an available configuration
option. Query-time exact/HNSW selection remains available. No build-strategy
setting should be exposed until the publication and recovery gates below pass.

## Current implementation and constraints

Local SQLite maintains message vectors and packed `semantic_chunks`; these are
storage batches, not an HNSW graph or independent retrieval passages. Local
dense retrieval is exact. Qdrant creates one named dense vector and one sparse
lexical vector per message. Its collection-wide HNSW topology is fixed at
`m=16`, `ef_construct=200`. Query-time `ef_runtime` neither rebuilds nor changes
that topology, and exact queries do not disable graph construction.

`qdrant_index::collection` currently derives only two collection names from a
group: state and messages. Every visible and staged revision shares those names.
`api::initialize` verifies existing schemas, including topology, rather than
silently patching them. A differing topology is rejected for both exact and
HNSW query defaults. Its error currently suggests a new group; that is an
existing limitation, not the intended configuration workflow.

`configuration::select_gold` atomically replaces the group's typed recipe and
epoch while preserving node references. Individual node gold rebuilds follow.
Related search rejects incompatible node manifests during that transition;
independent lexical search remains usable. This is durable authority and fencing,
but not staged all-node replacement with uninterrupted related search. The
remaining activation work is `mash-q3e.3.5`.

Durable candidate ownership is now available through `serve --stage-gold`, with
an authenticated `/v1/gold-candidate` report. This pins the active epoch and exact
accepted node/silver set while preserving active search. It stages metadata
only; candidate vectors, completeness checks and atomic all-node activation are
still required by `mash-q3e.3.5.3`. See the
[operator contract](silver-gold-configuration.md#durable-shared-gold-candidates).

An in-place collection PATCH would change construction for the active revision
and every candidate simultaneously. A new collection alone is also insufficient:
exact search, context viewing and obsolete-version cleanup currently address
the single messages collection. Thread metadata is in the state collection and
must remain bound to the same accepted node references. Repointing only dense
search would lose other retrieval paths or return false empty results. A failed rebuild
must not make current search depend on a partially prepared collection.

## Target contract

Gold build configuration will declare typed physical construction capabilities.
The default remains local packed exact vectors and Qdrant HNSW 16/200. Initial
explicit choices should be Qdrant flat vectors and bounded HNSW topology. Local
HNSW is unsupported unless separately implemented; reject it before opening or
modifying SQLite. Do not turn topology settings into ignored local knobs.
Quantization is a later measured capability, not an arbitrary pass-through map.

Graph construction belongs in build identity; exact/HNSW execution and runtime
`ef` belong in query policy. An HNSW request against a flat-only artifact must
fail with a typed capability error. URL, credentials and memory/deployment policy
remain outside the retrieval recipe. Query overrides never create a graph.

Prepared physical generations need distinct collection addresses. A typed
physical location must bind topology/version, vector dimensions/model space and
the required schema, with a deterministic legacy-default location. Preserve
source-free reads of canonical state independently of these addresses. Record
the accepted location with each published node reference; absence is the verified
legacy location, not whichever collection a process happens to prefer.
Topology-only replacement should reuse unchanged immutable canonical chunks
where their encoded content permits it. Separate canonical chunk identity from
physical document location instead of duplicating canonical bodies gratuitously;
cleanup must account for retained chunk references as well as graph ownership.

The group activation protocol must then:

1. Pin the active catalogue epoch, complete node set and each node's canonical
   silver revision. Maintain the existing active configuration during staging.
2. Build candidate vectors/documents and payload indexes in the candidate
   physical generation from retained silver. Reuse model-compatible vectors for
   topology-only changes; do not unnecessarily rerun inference or provider reads.
3. Verify each candidate's schema, coverage, silver/model/build identity and
   required readiness. Qdrant accepting desired configuration is distinct from
   completing background graph optimization; report that distinction explicitly.
4. Publish the new configuration and complete node-reference set in one fenced
   catalogue CAS. A new node, changed silver, forget or competing activation must
   invalidate or rebase the candidate; never commit a mixed node generation.
5. Resolve every search/view/thread path through accepted physical locations.
   Preserve exact lexical reads of the active generation while candidates build.
   Existing API processes must fail closed when their selected recipe is stale.
6. Clean superseded physical versions only after acknowledgement and after
   in-flight readers are fenced or can retry. Cleanup needs both the previous
   and candidate location; failed candidate cleanup must never address active
   data. Interrupted or uncertain writes require observation before cleanup.

A candidate is temporary construction state, not a second maintained profile.
Only one configuration is active. Failed staging preserves the active node set,
history and search behavior; restart can resume or discard the candidate using
durable ownership/epoch evidence. Empty groups, lexical-only gold and partially
certified source history need explicit behavior rather than fabricated readiness.

## Executable and measurement gates

The first guard checks current topology rejection at the typed REST boundary.
A disposable Qdrant test changes only its owned fixture's graph to 32/400 and
verifies that reconnecting with either query policy rejects it without restoring
16/200, republishing the node or changing stored history. These tests establish
the current boundary, not runtime protection against external administrators
altering a collection after startup or completed physical replacement.

Before exposing strategy configuration, differential lifecycle fixtures must
exercise flat/HNSW and multiple topologies, ordinary and filtered search, exact
mentions, thread metadata/view, provider/node/time exclusion, topology-only
vector reuse, source-free replacement and unchanged canonical fingerprints.
Include different-node concurrent publication, edits/deletes and new arrivals
during staging, competing API processes, CAS uncertainty, SIGKILL/restart,
optimizer completion, failed cleanup and legacy location migration.

Measure complete build and activation time, graph-ready lag, storage including
temporary generations, CPU/RSS, and filtered/unfiltered exact/HNSW recall and
latency on real sessions. Retain the current default until evidence supports a
change. The existing query benchmarks do not establish these construction costs.
