# Shared candidate text compaction

The HTTP search boundary sends a message projection once, in `documents`, instead
of also repeating it in lexical and semantic hits. A hit's inline `content` is
emptied only when an identical redacted projection exists for its node/message
identity. Clients already restore projections before eligibility, fusion,
context lookup and reranking. Scores, ordering, metadata and candidate budgets
are unchanged. Exact-mention responses have no documents and retain inline text.
Missing or unequal projections also retain inline text. Redaction still applies
to every outgoing text and metadata field.

## Live-data comparison

Use the same private buffered ARM64 client from the earlier
[buffering experiment](2026-10-05-shared-search-buffering.md). Compare the live
6977a8e API with a private LLVM release built from 4f42171 plus the compaction
changes, on a separate authenticated port using the same Qdrant collections.
Both API processes and the client enable tracing. Run baseline, compacted,
compacted, baseline, each in a fresh stdio MCP session. Each session searches
three queries (`database migrations`, `GitHub Actions CI`, `React testing`),
unfiltered and Codex-filtered, twice each, requesting three combined thread
results. No ingestion, publication, optimizer changes or cache flushing occurs.

| Session | Median core latency | Median shared candidates |
| --- | ---: | ---: |
| Baseline A | 2,004.0 ms | 988.1 ms |
| Compacted B | 1,999.5 ms | 958.3 ms |
| Compacted B2 | 2,231.5 ms | 1,127.3 ms |
| Baseline A2 | 2,180.5 ms | 1,163.8 ms |
| Baseline pooled, 24 calls | 2,156.5 ms | 1,066.3 ms |
| Compacted pooled, 24 calls | 2,039.5 ms | 992.7 ms |

All 48 calls succeed without shared fallback. Both compacted sessions match all
twelve A result objects exactly, including supporting messages and scores. The
pooled median core difference is 117 ms (5.4%), but session variation is material;
this small live-network comparison does not establish a reliable fleet speedup
or tail improvement. Decoded candidate bodies decrease by approximately 25–41%
over these queries.

Separately, send both APIs an identical request with one frozen, stored corpus
dense vector and the lexical query `database migrations`. This controls candidate
payload comparison; it is not an evaluation of the query embedding's relevance.
Request gzip directly and save the compressed bodies before decoding:

| Frozen-vector response | Baseline | Compacted |
| --- | ---: | ---: |
| Compressed HTTP body | 848,196 bytes | 492,068 bytes |
| Decoded JSON | 2,194,025 bytes | 1,283,297 bytes |

Compressed body size decreases 42.0%; decoded size decreases 41.5%. Reconstructing
the hit contents yields identical full candidate objects. Publication identities
remain unchanged throughout. The bandwidth saving supports retaining the bounded
change even though the latency estimate remains noisy.

An earlier pilot without tracing on the private API is retained privately but
excluded from this comparison. These runs do not resolve the intermittent native
dense stall or publication staging tail.

## Validation

634 standard tests pass, followed by the two final typed compaction/parity tests,
strict all-target Clippy, formatting and doctests. Two disposable Qdrant HTTP
authorization/redaction contracts pass on a 4 GiB tmpfs fixture. Initial disk
fixture setup times out while the ingestion suite/build runs with substantial
host I/O pressure; a subsequent 512 MiB tmpfs fixture fails because default WAL
allocations exhaust that budget. Those failures are not counted as passes.
The final fixture validates API behavior, not disk restart durability. All owned
disposable containers are removed after terminal test handles.

Private requests, responses, vectors, traces and logs live outside Git in
`/home/james/scratch/mash-artifacts/shared-tail-20261005`.
