# Coverage audit: 2026-10-02

The clean LLVM-instrumented suite passed all **676 selected tests** on Xenia,
including installed local embedding/reranking models, disposable PostgreSQL 17
with pgvector, Redis 8.2 Search, Qdrant 1.19.1, and real API/database restart
contracts. Sixteen manual benchmarks were excluded. Test execution took 225.351
seconds, excluding compilation, profile settlement and export. This is not a
search-latency or ingestion-throughput measurement.

Runtime Rust source was revision `dc0328c7599504ca78c0d21ccb5c9801cf2db2f9`.
This change updates only the coverage workflow and documentation. Tooling was
cargo-llvm-cov 0.9.1 and rustc 1.100.0-nightly (`fb6531d55`, 2026-08-23), using
`RUSTFLAGS=-Znext-solver=no`, `CARGO_INCREMENTAL=0`, no compiler wrapper, nightly
`--branch`, and `/home/james/scratch/achi-targets/coverage` as the persistent target.
Ordinary development continues to use Cranelift.

## Settled scope and results

| Measure | Covered / total | Coverage |
| --- | ---: | ---: |
| Lines | 21,204 / 23,144 | 91.62% |
| Branches | 2,877 / 3,750 | 76.72% |
| Regions | 30,443 / 34,543 | 88.13% |
| Functions | 2,271 / 2,716 | 83.62% |

The report contains 143 handwritten source files. Every reported file exists
inside this repository; every maximum mapped line fits its current length,
allowing the EOF boundary. The audit records each file's SHA-256 and actual
mapping bound. Workspace coverage artifacts were cleaned before the full run.
Three detached search workers exited normally before export, allowing their
profiles to flush.

Generated bindings, separate test files, standalone test helpers, manual
benchmark modules, the test-only relevance evaluator and retired offline
prototypes were excluded in this historical run.
The first export also exposed two missed test-only helpers:
`semantic/evaluation/cleanup.rs` and `semantic/embeddings/cpu/sensitivity.rs`.
Their enclosing modules are test-only. The final filter removes both, re-exporting
the same settled profiles without rerunning tests. Both exports and mapping
audits are retained. Earlier reports used different source/exclusion scopes;
their percentages are not a controlled trend comparison.

Tests embedded in production files still contribute to their totals. Experimental
branch coverage is not MC/DC. Linux execution does not establish native macOS
Core ML, WSL, live provider UI fidelity, fleet performance or semantic relevance
on independently reviewed real queries.

## Workflow and setup correction

`make coverage` now rejects a missing `MASH_TEST_SERVER_BINARY` before cleaning
or building. The real database restart contract needs the actual API executable,
not just backend URLs. The README documents the instrumented binary's target
path, Docker prerequisites and Xenia's `MASH_TEST_DOCKER_SG=1` setting. The missing
prerequisite rejection was checked directly, and the full run exercised the
successful setup.

The first attempt had a harness mistake: its plain local PostgreSQL URL omitted
`sslmode=disable`. In-process tests use `NoTls`, while standalone server startup
requires TLS unless explicitly disabled. That attempt stopped after 632 passes,
one PostgreSQL CLI startup failure and 43 unrun tests. Its log and run manifest
are preserved; all its owned containers were removed. With only the test URL
corrected, the exact failing instrumented CLI control passed in 25.914 seconds.
The subsequent full run cleaned those profiles and passed all 676 tests. No
production TLS behavior was relaxed.

The restart contract verifies acknowledged data after graceful database restart
and SIGKILL process termination on all three backends, using their persisted
storage. It does not establish power-loss durability or failover. API fixture
processes are forcibly terminated during replacement/cleanup, which can prevent
their final LLVM profile flush. Thus runtime contract checks can execute paths
that remain uncovered in the exported report. Successful execution and recorded
coverage are distinct evidence; the low server-setup percentage is not proof
that setup never ran.

## Material behavior to review next

| Source | Uncovered lines | Line coverage | Branch coverage |
| --- | ---: | ---: | ---: |
| `app/shared/backend.rs` | 118 | 86.39% | 58.20% |
| `app/shared/server/store.rs` | 104 | 51.17% | 50.00% |
| `app/shared/snapshot/plan.rs` | 78 | 76.85% | 62.50% |
| `app/ingestion/copilot/chat/replay.rs` | 63 | 71.10% | 56.45% |
| `app/ingestion/openclaw.rs` | 54 | 76.62% | 63.64% |
| `app/semantic/embeddings.rs` | 49 | 80.32% | 63.33% |

Source inspection identifies PostgreSQL pool/TLS setup, malformed snapshot
validation and native replay mutation branches as relevant review scopes. Some
zero-count regions are generic instantiations or lost child-process profiles;
inspect actual behavior and fixtures before interpreting them as untested paths.
Do not add tests just to raise a percentage.

The concrete startup failure also exposed an observability gap:
`backend::connection` collapses structured pool errors into one generic message.
Follow-up `mash-c0d.9.4` tracks bounded, typed failure categories and safe hints,
with no raw DSNs, credentials or backend error text. Correct cause classification
must use structured evidence or an explicit unknown fallback.

## Evidence and remaining gates

Final evidence is outside Git at
`/home/james/scratch/mash-artifacts/coverage-dc0328c-final-20261002/`:
`coverage.json`, `coverage.log`, `final-export.log`, `source-mapping-audit.json`,
the first export/audit, the focused correction log, backend logs/state and
`run.json`. The manifest pins source HEAD and backend image IDs, records both
successful exits, and reports no cleanup errors. A Docker ownership-label check
confirmed no harness containers remain. No production indexes were involved.

The earlier failed attempt is retained at
`/home/james/scratch/mash-artifacts/coverage-dc0328c-20261002/`.
Reproduce using the README coverage procedure with explicitly installed models,
disposable services and the instrumented API binary.

Hosted CI for this source revision still did not start: run `37060140280` had
zero Rust steps, with GitHub's billing/spending-limit annotation, and deployment
was skipped. Exact-head hosted validation and all-node deployment remain open.
This local coverage run does not satisfy those external gates.

Tracking: `mash-c0d.8.19` completes this measurement/workflow slice;
`mash-c0d.8`, native-provider compatibility, real-corpus relevance,
fleet latency and exact-revision deployment remain open.
