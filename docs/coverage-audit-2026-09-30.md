# Coverage audit: 2026-09-30

The LLVM-instrumented suite passes with explicitly installed local models,
disposable Redis 8.2 Search and PostgreSQL 17 with pgvector. Reproduce it with
`make coverage` and the three test environment variables documented in the README.
Regular development and targeted validation continue to use Cranelift.

## Historical initial measurement

The initial baseline was commit `e30b536`. The startup-contract measurement
added three CLI server contracts without changing production Rust code.

| Measure | Baseline | With startup contracts |
| --- | ---: | ---: |
| Passing tests | 309 | 312 |
| Production-file lines | 14,935 / 16,912 (88.31%) | 15,074 / 16,912 (89.13%) |
| Regions | 21,489 / 25,235 (85.16%) | 21,665 / 25,235 (85.85%) |
| Functions | 1,552 / 1,955 (79.39%) | 1,568 / 1,955 (80.20%) |
| HTTP server lines | 276 / 524 (52.67%) | 404 / 524 (77.10%) |

Three manual benchmark tests are excluded. Generated bindings and separate test
files are excluded from the report. Inline tests still contribute to their source
files' percentages. Branch coverage and MC/DC are not measured. These figures
are evidence of executed code, not proof of search quality, throughput, bounded
memory, native provider compatibility or production readiness.

The updated instrumented test run took 43.653 seconds, excluding compilation and
report generation. This is test-suite duration, not search latency.

## Behavior covered by this change

CLI startup rejects non-UTF-8 or oversized server configuration, unsupported
versions, empty node lists, invalid resource limits and invalid embedding spaces
before connecting to a backend. Credential tests cover missing, malformed and
duplicate tokens, duplicate node identities, invalid credential environment
variable names and Redis/PostgreSQL credential-setting mixups. Errors retain the
typed configuration-error contract and do not expose supplied token values.
Existing JSON/unknown-field rejection tests remain the source of those checks.

CI previously selected ignored backend tests by the narrower substrings
`postgres_publication` and `redis_publication`. The broader backend expressions
select 27 contracts, including 13 previously omitted native-provider, branch,
filter, WAL and Redis-repair contracts. All 27 passed locally. CI now also triggers
on pushes to `main`. Deployment now depends on successful CI for the caller's
commit and checks the current main SHA inside its serialized deployment lock.
The GitHub account billing restriction still blocks success-path all-node
verification in `mash-c0d.8.2`; a rejected CI job correctly skips deployment.

## Clean current measurement

The source state at `fc1bc7d` passes 338 tests under LLVM instrumentation;
three manual benchmarks are excluded. Test execution took 49.187 seconds,
excluding compilation and reporting. The clean report measures:

| Measure | Current |
| --- | ---: |
| Handwritten source-file lines | 15,498 / 17,340 (89.38%) |
| Regions | 22,149 / 25,704 (86.17%) |
| Functions | 1,612 / 2,001 (80.56%) |

An initial refresh in the persistent target mixed older workspace binaries:
`core.rs` had coverage mappings through line 907 even though its current length
was 137 lines. That mixed report is invalid as a current measurement. The
coverage task now runs `cargo llvm-cov clean --workspace` first. The corrected
run has no reported source mappings beyond current file lengths; `core.rs`
ends at line 137 in the report. Standalone `test_support.rs` and the test-only
relevance evaluator are now excluded too. Historical figures above use the
earlier report scope and should not be used to claim a percentage improvement.

Inline tests remain included, and branch/MC/DC coverage remains unmeasured.
The corrected JSON report and execution log are retained outside Git at
`/home/james/scratch/mash-artifacts/coverage-clean-20260930.{json,log}`.
Gemini checkpoint recovery has 64/64 covered lines; language validation has
65/65 including its inline tests. These counts support the executed regressions,
with live provider capture and broader search quality still requiring evidence.

## Current remaining priorities

| Source | Uncovered lines | Line coverage |
| --- | ---: | ---: |
| `app/semantic/models.rs` | 125 | 61.54% |
| `app/shared/server.rs` | 120 | 77.10% |
| `app/shared/backend.rs` | 118 | 85.82% |
| `app/shared/redis_index/canonical.rs` | 81 | 75.68% |
| `app/shared.rs` | 79 | 88.98% |
| `app/shared/direct.rs` | 75 | 80.16% |
| `app/ingestion/openclaw.rs` | 54 | 77.02% |
| `app/semantic/remote.rs` | 51 | 84.11% |

Review exact uncovered paths before choosing tests. Prioritize safe model-install
failure handling, backend lifecycle and recovery, canonical Redis validation,
and first-run error behavior. Use controlled fixtures and local fault injection;
do not download models implicitly or mutate production indexes to raise coverage.
Fresh cold/warm local and combined latency, labeled query relevance, deployment
verification and real-provider UI compatibility remain separate open gates.

The reproducible command and startup contracts complete this measurement slice.
The broader audit remains tracked as `mash-c0d.8`.

## Model metadata follow-up

Follow-up `mash-c0d.8.3` bounds installation marker, revision and tokenizer reads,
validates inventory paths/counts and local dimensions, and publishes markers with
unique atomic staging files. Remote readiness checks the actual reranker
repository component. Asset validation and discovery now live in
`app/semantic/models/assets.rs`, separately from model execution and caching.

Eight metadata tests exercise corrupted/incomplete assets and failure recovery.
The subsequent LLVM run passed 320 tests (three manual benchmarks excluded),
including real installed models and both disposable backends. A later, additional
CLI test passed with Cranelift and verifies oversized metadata yields actionable
status and model errors without database creation or model-directory mutation.
That CLI test is not part of the following instrumented measurement.

| Measure | Model metadata follow-up |
| --- | ---: |
| Production-file lines | 15,171 / 16,991 (89.29%) |
| Regions | 21,806 / 25,333 (86.08%) |
| Functions | 1,576 / 1,962 (80.33%) |
| Asset boundary lines | 155 / 185 (83.78%) |
| Model execution/cache lines | 200 / 325 (61.54%) |

The instrumented run took 48.726 seconds excluding compilation and reporting.
The same exclusions and interpretation limits above apply. Model download/load
failure handling and platform-specific Core ML behavior still require stronger
evidence; a tested metadata boundary does not close those broader gates.
