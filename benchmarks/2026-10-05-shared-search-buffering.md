# Shared search response buffering

The shared client now buffers search responses up to 2 MiB before selecting
streaming JSON decoding. Other interactive endpoints retain their 1 MiB preview;
state, silver and publication-plan reads always stream. The eight interactive
admission slots bound accumulated preview buffers to 16 MiB. This is not a bound
on decoded records or on an additional received chunk that triggers streaming.
Deadlines, cancellation, admission permits and typed decoding are unchanged.

## Experiment

On 2026-10-05, compare the installed dev Mac binary (bc0dd6d) with a private ARM64
release built from 45bc57f plus the buffering patch. Commits between those bases
only changed documentation. The private binary was explicitly marked as a dirty
experiment; the installed binary was not replaced during measurement.

Run four fresh stdio MCP sessions in baseline, buffered, buffered, baseline order.
Each session searches `database migrations`, `GitHub Actions CI` and `React
testing`, both without a provider filter and restricted to Codex, twice per
combination. Each search requests three thread results, combined local/shared
scope and diagnostics. Both versions use the same live corpus, native models,
API and HNSW configuration. No indexing, publication, cache flushing or Qdrant
restart is performed. This is a small live-network comparison, not a forced-cold
benchmark or a fleet percentile estimate.

| Session | Calls | Median core latency | Median shared candidates |
| --- | ---: | ---: | ---: |
| Baseline A | 12 | 2,277.5 ms | 1,195.8 ms |
| Buffered B | 12 | 2,031.5 ms | 1,017.0 ms |
| Buffered B2 | 12 | 1,895.5 ms | 955.2 ms |
| Baseline A2 | 12 | 2,219.0 ms | 1,215.5 ms |
| Baseline pooled | 24 | 2,228.5 ms | 1,200.2 ms |
| Buffered pooled | 24 | 2,031.5 ms | 1,003.5 ms |

Pooled median core latency decreases by 197 ms (8.8%); median shared-candidate
latency decreases by 196.6 ms (16.4%). All 48 calls succeed with no shared fallback.
The second buffered run's complete result objects match all twelve first-baseline
result objects, including scores, metadata and supporting messages. One result
set varies between the two buffered runs, so live results are not universally
identical even within the same version.

Candidate replies are approximately 0.4–1.6 MiB after decompression. Six of twelve
first-baseline candidate replies cross the old preview threshold. Their streaming
decoder spans range from 23.3–158 ms; buffering reduces the corresponding first-B
decode spans to 2.2–6.8 ms. Streaming spans include blocking network waits: this
does not establish an equivalent CPU saving. Total request measurements, rather
than the decoder span alone, justify retaining the small bounded change.

The earlier multi-second native dense-query stalls did not recur in this
comparison and remain unresolved. Transfer/scheduling still contributes material
latency. Repeated projection text in the candidate payload is a separate
optimization candidate (mash-c0d.2.7).

## Validation and artifacts

The standard suite passes 632 tests (124 skipped). Final focused body tests pass
five tests, covering threshold selection, typed fixed/chunked/gzip decoding and
cancellation. Strict all-target Clippy, formatting and doctests pass. Real session
responses, traces, the source patch and build logs remain private outside Git in
`/home/james/scratch/mash-artifacts/shared-tail-20261005`.
