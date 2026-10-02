# Shared response decoding and search regression check

`mash-c0d.8.20`, Xenia, 2026-10-03.

The shared HTTP client previously awaited `reqwest::Response::json`, which
buffers the response and deserializes it while polling the async task. Large
candidate, context or node-state responses could occupy that runtime worker
while unrelated MCP reads, upload heartbeats and watch refreshes needed it.

The client now receives bytes asynchronously and decodes the same owned typed
models through the application's existing blocking boundary. That boundary
propagates the tracing subscriber, parent span and progress context. Status and
malformed-response errors retain their sanitized messages. No response text,
URL, history or credentials are added to traces.

The `shared_http_decode` child span records byte count and success/error. Its
timing covers work on the blocking thread; the enclosing HTTP span's elapsed
field includes I/O, blocking-worker queueing and decoding. This helps distinguish
decode work from the overall request without changing the wire protocol.

## Checks

A real loopback HTTP fixture runs on a single-thread Tokio runtime. A typed
test decoder signals entry, records its thread identity and waits for release.
Before releasing it, the test completes a second HTTP request and verifies the
returned typed counts. The first decoder remains held throughout. A watchdog
prevents a regressed implementation from hanging the suite. This demonstrates
runtime responsiveness under a deliberately held decoder; it is not a throughput
benchmark or a simulation of model inference.

The trace fixture checks successful and malformed JSON responses, inherited
HTTP trace IDs across the blocking boundary, decode outcomes/byte counts,
absence of response canaries and disabled tracing. Existing status, timeout,
publication and local-fallback controls remain part of the standard suite.

Three additional application/API contracts use a disposable Qdrant 1.19.1
container: authorization/groups/vector publication/rollback/retry; core thread
catalog deduplication/replacement/deletion/forget; and real-model scoped message
search using thread discovery. The owned container was removed after the test
process completed. Its image was
`sha256:12364fe851b9f17356fc88189fc06d1b521262e04659ec7345975b00c9246a10`.
These checks cover Qdrant HTTP paths; PostgreSQL/Redis service integration was
not rerun for this transport-only change.

The fourth opt-in control reruns the local synthetic relevance matrix through
the application core with explicitly installed models. Fixture
`docs/fixtures/search-relevance.toml`, SHA256
`7485f9c59d5df0aa3a65ecfcb2a2816952e3a632f46a860c23effbfd6341cad3`,
contains 17 threads, 20 messages and 23 query cases: 16 positive and seven
abstention cases, each run in both message and thread modes. All first-result,
forbidden-result and abstention checks passed.

| Unit | Mean precision among returned results, up to 5 | Recall@5 | MRR@5 |
| --- | ---: | ---: | ---: |
| Messages | .896 | 1.000 | 1.000 |
| Threads | .885 | 1.000 | 1.000 |

These are audit-authored synthetic judgments, not independent human gold or
production-corpus relevance. Message relevance uses conversation membership,
not independently reviewed individual excerpts. The current fixture has more
cases than the September report, so these numbers do not establish a quality
improvement over that report. The four opt-in checks passed in 9.609 seconds;
that total includes fixture setup and does not establish end-to-end user latency.

Logs are outside Git under `/home/james/scratch/mash-artifacts`, with prefixes
`shared-decode-` and `search-relevance-current-20261003`.
The final standard Cranelift suite passed all 597 tests in 8.475 seconds, with
115 opt-in controls excluded. Strict all-target Clippy, formatting and diff
checks and the single doctest also passed. A test-only helper extraction keeps the trace assertions
separate from the HTTP fixture setup without suppressing the line-count lint.

## Remaining limits

Successful bodies and decoded responses are still buffered. This change does
not bound aggregate response memory, introduce paging or cancel already running
JSON decoding. The HTTP transport timeout is not a CPU-decoding deadline.
`mash-c0d.21.4` tracks endpoint-specific response work/backpressure while
preserving valid large histories; a generic arbitrary size cap or silent
truncation is not an acceptable substitute. Whole-project production readiness
and independently labeled search quality remain open.
