# Coverage audit: 2026-10-01

The clean LLVM-instrumented run of source revision `183583a` passed all 499
selected tests, including explicitly installed embedding/reranking models and
disposable PostgreSQL/pgvector and Redis Search contracts. Ten manual benchmarks
were excluded. Test execution took 93.562 seconds, excluding compilation and
reporting; this is not a search-latency measurement.

## Settled measurement

| Measure | Covered / total | Coverage |
| --- | ---: | ---: |
| Lines | 18,952 / 20,764 | 91.27% |
| Branches | 2,488 / 3,272 | 76.04% |
| Regions | 27,392 / 31,212 | 87.76% |
| Functions | 2,006 / 2,419 | 82.93% |

The report contains 123 handwritten source files. Every reported file exists,
and its maximum mapped line fits its current source length, allowing the EOF
boundary. Generated bindings, separate tests, standalone test helpers, the
test-only relevance evaluator, benchmark modules and the indexing memory
benchmark helper are excluded. Tests embedded in production source files still
contribute to their files' totals. Consequently these figures are not isolated
production-only coverage.

Branch instrumentation uses nightly LLVM's experimental `--branch` support.
MC/DC was not measured. This Linux run does not establish native macOS/Core ML,
WSL or live provider UI behavior. Earlier reports used different source and
exclusion scopes; their percentages are not a controlled trend comparison.

## Reproduction and profile lifecycle

Set `MASH_TEST_MODELS_DIR`, `MASH_TEST_PG_URL`, and `MASH_TEST_REDIS_URL` to the
explicitly installed assets and disposable services documented in the README,
then run `make coverage CARGO_ARGS=--branch`. Coverage requires LLVM; ordinary
development continues to use Cranelift. On the installed August nightly this
run used `RUSTFLAGS=-Znext-solver=no`, `CARGO_INCREMENTAL=0`, and the persistent
`/home/james/scratch/achi-targets/coverage` target.

Two test-only benchmark helpers remained in the original report filter. The
filter now excludes both directory-form benchmark modules and
`app/storage/indexing/memory.rs`. Reporting again from the same profiles fixes
the scope without rerunning the tests.

The first export also preceded detached CLI workers' normal idle exits. Their
later profile flushes added 11 covered lines and one branch. The settled report
was generated after no matching coverage-target worker remained; subsequent
totals were identical. `make coverage` now waits for its user's workers from the
selected Cargo target before cleaning and before reporting. It preserves their
normal idle exit and aborts if they remain active beyond 125 seconds. Workers
from other targets are ignored. Real process checks verified target isolation,
paths containing spaces, timeout failure and normal exit.

The settled JSON and execution log are retained outside Git:

- `/home/james/scratch/mash-artifacts/coverage-183583a-branch-settled.json`
- `/home/james/scratch/mash-artifacts/coverage-183583a-branch.log`

## Remaining behavioral review

| Source | Uncovered lines | Line coverage | Branch coverage |
| --- | ---: | ---: | ---: |
| `app/shared/backend.rs` | 119 | 86.27% | 58.20% |
| `app/shared/server.rs` | 117 | 78.49% | 80.56% |
| `app/shared/redis_index/canonical.rs` | 80 | 76.05% | 60.58% |
| `app/ingestion/copilot/chat/replay.rs` | 63 | 71.10% | 56.45% |
| `app/ingestion/openclaw.rs` | 54 | 76.52% | 63.64% |
| `app/semantic/remote.rs` | 42 | 86.92% | 61.84% |

Review exact uncovered behavior before adding tests. Priorities are backend
recovery and canonical decoding, provider replay fidelity, and bounded remote
embedding failure handling. Use local fault injection and recorded upstream
fixtures; do not mutate production indexes or add tests solely to raise a
percentage. Coverage does not establish relevance, throughput or robustness
under concurrent production traffic.

This measurement and worker-profile fix are tracked by `mash-c0d.8.16` and
`mash-c0d.8.17`. The broader audit (`mash-c0d.8`), live-provider compatibility,
labeled relevance evaluation, fleet latency goals and exact-revision deployment
verification remain open.
