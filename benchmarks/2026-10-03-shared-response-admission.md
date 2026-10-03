# Shared response admission and deadlines

`mash-c0d.21.4.1`, Xenia, 2026-10-03. This closes a concurrent-work gap in the
shared HTTP client; it is not the complete response memory/paging solution.

The client previously received arbitrarily many bodies and started blocking
decoders as concurrent requests arrived. Moving JSON decoding off the runtime
worker improved responsiveness, but cancellation could abandon that work while
new requests kept arriving. HTTP deadlines also omitted time waiting for a
blocking decoder. Large node checkpoints and canonical contexts remain valid
data; an arbitrary universal byte cap would reject legitimate histories.

## Change

One process-wide admission pool allows eight interactive requests and one bulk
request in separate lanes. Eight preserves the existing context-retrieval
concurrency. State, index publication and forget use bulk capacity; freshness,
search, thread metadata and context use interactive capacity. Acquire capacity
before sending HTTP, so waiting callers do not receive/buffer bodies.

The existing endpoint deadline now covers admission, HTTP I/O and waiting for
typed decoding: two seconds interactive, 600 seconds bulk. A decoder owns its
permit until it actually finishes, even when the caller cancels or times out.
Cancellation before decoding drops the request/body and returns capacity.
Already-running serde work is not interrupted. A runtime shutting down can
still wait for that work; this is not a CPU-execution or CLI-process time limit.

`shared_http_request` records queue milliseconds, the fixed endpoint name and
admission class alongside correlated trace ID, status, outcome and elapsed time.
Queue duration is recorded on admission expiry as well as success. Decode spans
retain byte count, outcome and request ancestry. URL, credentials, query and
body contents remain excluded; errors retain their sanitized shared-index
envelope and local fallback behavior.

## Validation

Three new fixtures use private admission pools so concurrent tests do not
consume each other's global capacity. They verify:

- Admission expiry sends no HTTP request and returns the exact sanitized,
  retryable timeout, without query/credential canaries.
- Exhausted bulk capacity leaves an interactive read available.
- Both explicit cancellation and deadline expiry retain a held decoder's slot;
  capacity returns only after release and actual worker completion, and a
  subsequent read succeeds.

The six focused tests, including existing single-runtime responsiveness and
client/server correlation controls, passed in 2.027s. The queue-expiry and held
decoder tests intentionally exercise the real two-second deadline; these are
test durations, not production search latency measurements. Correlation
assertions subsequently added for queue/endpoint/class fields passed in the
final full suite: 602 tests in 8.040s, with 115 opt-in cases excluded.

Four opt-in integration checks passed in 11.903s:

- Qdrant HTTP authorization, vectors, node groups, rollback and retry.
- Shared core thread catalog deduplication, replacement, deletion and forget.
- Shared core thread discovery followed by scoped message search, with installed
  local embedding/reranking models.
- Shared candidates requested while local SQLite retrieval is deliberately
  blocked, after inference preparation.

The service was disposable Qdrant1.19.1, preloaded image
`sha256:12364fe851b9f17356fc88189fc06d1b521262e04659ec7345975b00c9246a10`.
Its owned container `908847fc62dbadc7c1fc6532fc898f06f87102e39396effdf18404ba393fef9a`
was removed after terminal test completion; its owner-label listing is empty.
No model download, installed daemon, remote deployment or real PostgreSQL/Redis
service run is claimed. Strict all-target Clippy passed in 23.22s; formatting and
whitespace checks passed. Workspace doctests passed (one application example,
no query-crate examples). Test/lint/doctest artifacts are retained under
`/home/james/scratch/mash-artifacts/shared-admission-*`.

## Remaining limits

This bounds in-flight response work per process, not body bytes, retained typed
results, pending callers, backend work across processes, or complete fleet
memory. A single huge body can still exhaust memory. The parent
`mash-c0d.21.4` remains open for endpoint-specific paging/body budgets, agreement
with large-history server semantics and realistic multi-node measurements.
No indexing/search algorithm, quality target or backend data representation was
changed. No latency or memory speedup is inferred from these correctness checks.
