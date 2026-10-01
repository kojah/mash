# Shared snapshot reads and replacement memory

The shared Qdrant and Redis adapters now receive canonical JSON chunks into a
private, checksummed temporary file instead of assembling the full JSON in RAM.
The file is decoded on a blocking worker. For index and forget mutations,
decoding and mutation planning execute in one blocking job, so freeing old
snapshot objects and constructing their replacements happen on the same worker.
Redis catalog reads use the same verified-file boundary.

Length limits, checksums, active-revision checks, canonical context, and
conditional publication remain enforced. A received file cannot be decoded
through this boundary until its complete length and digest have been verified.
The validated Rust data model remains resident in memory; this is not a process
RAM budget or a solution to concurrent publisher/backend memory limits.

## Real-history measurement

The benchmark used the frozen accepted contributions from the preceding
[publication measurement](2026-10-01-real-shared-publication.md): 789 threads,
225,267 canonical messages, and 29,453 compatible 384-dimensional vectors from
Claude Code and Codex. Each fresh production `mash serve` process first published
revision 1, then received a complete revision-2 replacement with one synthetic
thread-title change. Messages, projections, vectors, and context were unchanged.
All six publications returned HTTP 200 with matching counts and expected
revisions. The final implementation also returned the new title through the
authenticated thread catalog API.

| Implementation | Observed API peak RSS across both phases | Initial publication | Replacement |
| --- | ---: | ---: | ---: |
| `fdf91e1` baseline: full input JSON buffer | 1,296,596 KiB | 85.417 s | 123.263 s |
| Experiment: file reader, separate decode/planning jobs | 1,375,040 KiB | 92.126 s | 134.296 s |
| Final: verified file, decode/planning in one job | 763,296 KiB | 133.645 s | 133.161 s |

The first experiment did **not** reduce observed peak RSS. Removing the JSON
buffer alone was insufficient in that run. Keeping decoding and planning on one
worker reduced observed peak RSS by **41.1%** against the baseline, approximately
521 MiB. The changed worker boundary permits freed allocations to be reused by
planning; the allocator's exact contribution was not isolated with a profiler.
This is a measured observation, not a guaranteed maximum across allocators,
platforms, corpus shapes, or concurrent requests.

Each run used an isolated group on Qdrant 1.19.1, cached placement, exact retrieval,
and a 4-core/2-GiB database container. The host API process had no memory limit.
The same original and replacement NDJSON bodies were reused for all runs, with
an explicit 2,048-MiB node cap. Host load was uncontrolled, and compilation or
tests overlapped portions of the runs. Timings do **not** establish a throughput
improvement; the final replacement was about 10 seconds slower in this sample.
File I/O and reader-based JSON decoding have costs that need workload-specific
measurement. No query inference, search latency, relevance, or ANN claim follows
from these mutation measurements.

Original SQLite and provider histories stayed read-only. The frozen corpus's
native refresh remains incomplete because of a malformed history record; no
readiness marker was changed or full-corpus recovery claimed.

## Validation and remaining work

All **551 tests passed** in final validation, including explicit real-model,
PostgreSQL, Redis, Qdrant, CLI, and MCP tests; 10 manual benchmarks were excluded.
Clippy with all targets/features, formatting, doctests, and whitespace checks
passed. The receiver regression covers exact Unicode decoding across binary
chunk boundaries, truncation, checksum corruption, length overflow, and
sanitized malformed-JSON errors. Existing live tests retain replacement,
competing publication, missing/invalid chunks, catalog integrity, and forgetting
coverage.

An earlier full-suite run had one five-second cleanup timeout while a large
benchmark collection was being deleted on the same Qdrant instance. Validation
was rerun on a separate disposable backend and passed. `AGENTS.md` now requires
separating full validation from real-corpus benchmarks and bulk cleanup.

Private artifacts in
`/home/james/scratch/mash-artifacts/shared-replacement-20261001` contain the
baseline binary, benchmark scripts, body hashes, publication responses,
per-process high-water RSS samples, comparison, collection counts, and cleanup
evidence. Benchmark and test collections were removed only after their handles
completed; the pre-existing smoke collection was preserved.

`mash-c0d.21` remains in progress for memory-based publication admission,
cancellation/reservation lifetime, concurrent publisher and reader measurements,
backend memory limits, and realistic Redis resource measurements. Temporary disk
and filesystem cache now carry input payloads; decoded data and backend staging
still need explicit capacity planning.
