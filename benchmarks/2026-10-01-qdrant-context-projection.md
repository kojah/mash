# Qdrant context retrieval without unrelated message bodies

Previously, `view` retrieved the anchor, scrolled complete stored documents for
the entire thread, then selected its context window. Unrequested canonical
bodies, search projections, and stored embedding arrays were retained until
selection completed. This was scoped to a thread, not the entire node snapshot.

The new implementation scrolls typed identity/ordinal/ancestry records using
[Qdrant payload field selection](https://api.qdrant.tech/api-reference/points/scroll-points),
selects the existing linear or branch-aware neighborhood, then retrieves complete
records for only that selection in batches of at most 100. Fetched identities,
ordinals, ancestry, scope, and active revision are checked before returning any
result. Canonical content, empty projections, and extensible roles are preserved;
selected body strings move into the response instead of being cloned. The
`select_shared_context` trace span records thread and selected-message counts,
without message text or identifiers.

## Real-history comparison

The baseline release binary is from `08087f1`; the changed release binary uses
the same storage layout and published data. The baseline SHA-256 is
`fb6a084afd4034157d76d38a2dd08ccdf3b28058850181223030fbeca79bb774`, and the
changed binary SHA-256 is
`e1a11fbd0be81f0e967bc49e0ceb9d4f1b2c9c1035f5c0e8f80a2b7e1d0c1e83`.

One dedicated Qdrant 1.19.1 service used cached placement, exact retrieval, four
CPUs and an 8-GiB container limit. The API process had no memory limit. An isolated
group received the existing frozen 550,139,311-byte upload: 789 conversations,
225,267 messages and 29,453 searchable messages/vectors. Publication succeeded
with revision 1. No storage schema or model changes were involved.

Three long Codex threads were selected by canonical character count:

| Thread messages | Canonical characters |
| ---: | ---: |
| 14,649 | 34,244,567 |
| 15,855 | 32,352,037 |
| 10,442 | 30,033,307 |

Each trial used fresh baseline and changed API processes against the same active
data. For each thread, its middle message was viewed with canonical context 0,
canonical context 2, and search-projection context 2, repeated three times:
**27 requests per implementation per trial**. Context-0 responses were checked
against the read-only SQLite source. Every changed response was byte-for-byte
identical to its corresponding baseline response, including the surrounding
messages. The database and machine caches were not reset between trials.

| Trial | Implementation | p50 | p95 | Peak sampled API high-water RSS |
| --- | --- | ---: | ---: | ---: |
| 1, builds/tests overlapped | Baseline | 939 ms | 1,221 ms | 188,392 KiB |
| 1, builds/tests overlapped | Changed | 462 ms | 2,641 ms | 45,944 KiB |
| 2, after our builds/tests completed | Baseline | 810 ms | 1,167 ms | 144,196 KiB |
| 2, after our builds/tests completed | Changed | 439 ms | 732 ms | 51,496 KiB |

In the second trial, measured API peak RSS was **64.3% lower**, and p95 latency
was **37.3% lower**. The first trial had changed-process startup outliers up to
3.07 seconds; its p95 regressed despite improved median latency and memory. Host
load was not controlled, and these are two sequential observations rather than
a randomized, statistically powered experiment. Overlapping compilation/tests
are recorded conditions, not a proven explanation for the outliers. Do not infer
guaranteed tail latency, throughput or an overall RAM bound from this comparison.

The corpus contains accepted contributions from an incomplete native refresh.
Provider histories and source SQLite stayed read-only; no readiness marker was
changed. These requests measure the HTTP `view` endpoint, not candidate search,
embedding inference, reranking, CLI startup or full agent workflows. The real
performance cases are linear Codex histories; branch correctness is separately
covered by live backend contracts and unit regressions.

## Validation and limits

All **561 tests passed**, including explicit real models, PostgreSQL, Redis,
Qdrant, CLI and MCP contracts; 10 manual benchmarks were excluded. Clippy with
all targets/features, doctests, formatting and whitespace checks passed.

Five unit regressions cover gapped linear windows, native branch paths, malformed
or duplicate topology, wrong scopes/versions, missing or changed fetched records,
anchor identity, opaque roles and empty projections. Live Qdrant contracts cover
branch/context-only fidelity and a 256-message thread spanning multiple topology
pages with 201 selected records fetched across three batches. An initial run
failed because this new fixture put 256 records into one upload batch, above the
existing 128-record protocol limit; the fixture now publishes two valid batches.
The final full suite passed without test retries.

Topology still scales with the number of thread messages. Large requested
windows still return their complete selected bodies. This removes unnecessary
body/vector retention, but does not add memory-weighted admission, bound all
concurrent context readers, eliminate all thread-wide work, or reclaim old
backend staging. `mash-c0d.21` remains open for those resource-budget requirements.

Private artifacts are under
`/home/james/scratch/mash-artifacts/shared-context-20261001`: baseline binary,
exact harness/configuration, both trials, response hashes, 50-ms API memory
samples, full-validation log and cleanup evidence. Benchmark and full-validation
services were separate. Their owned containers and anonymous volumes were
removed after their process handles completed; unrelated services were untouched.
