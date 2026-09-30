# Coverage audit: 2026-09-30

The LLVM-instrumented suite passes with explicitly installed local models,
disposable Redis 8.2 Search and PostgreSQL 17 with pgvector. Reproduce it with
`make coverage` and the three test environment variables documented in the README.
Regular development and targeted validation continue to use Cranelift.

## Measurement

Baseline is commit `e30b536`. The updated measurement adds the three CLI server
startup contracts in this change; it changes no production Rust code.

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
on pushes to `main`; deployment is still independent and must be gated on the
exact validated commit in the tracked follow-up `mash-c0d.8.2`.

## Remaining priorities

| Source | Uncovered lines | Line coverage |
| --- | ---: | ---: |
| `app/semantic/models.rs` | 173 | 59.86% |
| `app/shared/server.rs` | 120 | 77.10% |
| `app/shared/backend.rs` | 118 | 85.82% |
| `app/shared/redis_index/canonical.rs` | 81 | 75.68% |
| `app/shared.rs` | 79 | 88.98% |
| `app/shared/direct.rs` | 75 | 80.16% |
| `app/core.rs` | 65 | 89.95% |
| `app/ingestion/openclaw.rs` | 54 | 77.02% |

Review exact uncovered paths before choosing tests. Prioritize safe model-install
failure handling, backend lifecycle and recovery, canonical Redis validation,
and first-run error behavior. Use controlled fixtures and local fault injection;
do not download models implicitly or mutate production indexes to raise coverage.
Fresh cold/warm local and combined latency, labeled query relevance, deployment
verification and real-provider UI compatibility remain separate open gates.

The reproducible command and startup contracts complete this measurement slice.
The broader audit remains tracked as `mash-c0d.8`.
