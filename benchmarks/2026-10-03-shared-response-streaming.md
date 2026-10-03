# Shared response streaming control

## Implementation and scope

Node-state reads now decode directly from HTTP frames on a blocking worker.
Other successful responses retain the buffered `from_slice` path up to a 1 MiB
preview; a larger Content-Length or a chunk crossing that boundary selects
streaming. A 64 KiB `BufReader` feeds `serde_json::from_reader` through the existing
Tokio IO bridge. No new crate or version is introduced; existing tokio-util IO
and cancellation features are enabled.

This removes whole-wire retention, not the owned result. A transferred prefix,
one upstream chunk, HTTP buffers and the growing typed model still use memory.
There is no truncation or arbitrary history-size rejection. Paging and owned-data
budgets remain open under `mash-c0d.21.4`.

Cancellation wakes a worker blocked on the next body frame. Its admission permit
remains owned until actual completion, including CPU parsing that cancellation
cannot preempt. EOF, invalid schema, trailing bytes and transport errors prevent
returning a partial state. Decoder telemetry records `mode`; streamed child time
includes body IO waits, and its byte count includes reader read-ahead.

## Reproducible wire-copy control

The ignored `shared::client::body::tests::control::large_state_wire_memory_control`
accepts `MASH_TEST_STATE_URL`, `MASH_TEST_STATE_RECORDS` and
`MASH_TEST_STATE_MODE=buffered|adaptive`. Buffered reproduces the old complete
Reqwest Bytes plus `from_slice` behavior without adding a Bytes-to-Vec copy.
Adaptive exercises the new State path. Both hash the entire typed result through
a streaming BLAKE3 writer, avoiding another full serialized allocation.

The synthetic fixture contains 100,000 Codex checkpoints: ordinal zero-based
64-character hex path/fingerprint, ordinal size, i64::MAX minus ordinal modified
time, alternating readiness and conversation IDs of `thread-🧪-{ordinal}-` plus
64 repetitions of `payload `. Root instance is `wire-control`, revision i64::MAX,
and one Claude Code tombstone is `deleted-🧪`. Compact UTF-8 JSON is 79,827,925
bytes, SHA-256
`c74356779e28fa5da2501d1537005fd30459b34b595bf58d2daa28679ddc0b3e`.
The fixture and logs live outside Git in mash-artifacts. A separate loopback
Python HTTP server serves the frozen file; its memory is excluded from the client
measurement. Linux `memory_audit.py` samples the direct Cranelift libtest ELF at
20 ms intervals, excluding the observer and server. Its sampled RSS/PSS maxima
are lower bounds and monitoring can perturb timing. No rusage peak subtraction
or launcher-peak attribution is used.

Initial fresh-process controls, while validation builds were also running:

| Mode | Read + decode | Sampled peak RSS | Sampled peak PSS |
| --- | ---: | ---: | ---: |
| Old buffered | 1,292 ms | 197,476 KiB | 194,580 KiB |
| Adaptive State | 9,075 ms | 125,860 KiB | 123,237 KiB |

Both returned 100,000 records with identical typed BLAKE3
`ca8c0fb7cc3a7013dbd8f840debdc759d3b5ad807168f77aa76b831ce5779c73`.
These are memory controls with timing context, not controlled throughput results.
Streaming has a substantial parsing cost; it is a memory tradeoff for large bulk
state, not a search speedup. Small interactive responses retain the fast parser.
This fixture does not measure native ingestion, real large-history distributions,
PostgreSQL/Redis service behavior, MacBooks, multi-node memory or deployment.

The repeat used fresh processes after validation completed, with no Cargo/rustc
process live at the pre-run observation. Both arms used the same final
nonincremental Cranelift debug ELF; the initial pair used the same earlier
incremental debug ELF. Repeat buffered/adaptive read-and-decode times were
1,050/8,343 ms, sampled peak RSS 199,788/127,492 KiB and PSS
196,874/124,562 KiB (152/456 samples). All four runs returned the same complete
typed digest, and the source hash was unchanged. The repeat saves approximately
36% client RSS but takes about eight times as long to read/decode this fixture.
These development-build timings are not release benchmarks or search latency.
The observer includes typed digest generation and test shutdown in its memory
scope, while reported read/decode time excludes those stages.

## Correctness validation

Actual loopback HTTP tests check complete typed large-state equality for fixed
length and chunked bodies, the exact preview boundary and one byte beyond it,
decoding before EOF, cancellation returning capacity and upstream body cleanup,
truncated/trailing/schema failures, and a transport failure after valid JSON.
Existing admission, decoder concurrency and trace correlation checks remain.
Final current-source validation: 607 standard workspace tests passed in 8.896 s;
116 opt-in tests were excluded, including this four-run memory control. Four
explicit real Qdrant 1.19.1/local-model checks passed in 13.107 s: publication
authorization/vectors/groups/rollback/retry, thread catalog/dedup/replacement/
delete/forget, scoped message search and local/shared concurrency. The exact
owned container was removed after terminal test completion, and its owner-label
listing was empty. All-target strict Clippy, formatting, diff checks and the one
doctest passed. Initial control compilation needed a Result wrapper; strict
Clippy then required moving test items before statements and a clearer empty
assertion. Those failures were corrected and current-source checks passed.
No production deployment or PostgreSQL/Redis service validation is claimed.
