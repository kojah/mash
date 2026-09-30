# Test iteration timing on Xenia

Baseline source: `c1011e79566f22e3d2eb2033903d5f6b2f33835d`.
Host: x86_64 Linux, rustc 1.100.0-nightly (2026-08-23), Cranelift for mash,
LLVM for dependencies. Dependencies and installed model assets were warm.
Measurements used the persistent integrated target directory and test temp
directory documented in AGENTS.md. No models or services were installed during
the build measurements. These are individual observations, not p50/p95 results.

## Build observations

Commands used `cargo test-clif --no-run --timings`, selecting `--lib` for unit
measurements. Small temporary edits were made to a unit-test source, then fully
reverted. Build timing reports identify the library test binary as the critical
path; filtering tests does not reduce the compiler's work within that binary.

| Configuration | Wall time |
| --- | ---: |
| Default scope, incremental off, comment edit | 32.39 s |
| Unit-only scope, incremental off, comment edit | 34.79 s |
| Unit-only, mash debug level 1, incremental off, comment edit | 31.70 s |
| Unit-only, incremental on, wrapper disabled, first build | 41.73 s |
| Same incremental workflow, assertion-message edit | 8.59 s |
| Same incremental workflow, restored original source | 7.71 s |
| Unit-only, incremental off, wrapper disabled, restored original source | 34.13 s |

Unit-only scope and reduced debug information did not materially improve this
case. Incremental compilation reused work after a change to an assertion's
diagnostic string, as well as when restoring the source. This does not establish
the same improvement for new tests, public type changes, macro changes, dependency
updates, clean builds, or native Mac builds. The initial incremental cache used
approximately 455 MB after compiling mash and the generated PostgreSQL crate.

Enabling incremental compilation through this host's configured sccache 0.17.0
wrapper failed before completing the generated PostgreSQL crate. The same build
with `RUSTC_WRAPPER=` succeeded. The wrapper failure's underlying cause is not
established; it is not a claim about all sccache versions or configurations.
The successful incremental measurements retain normal debug information.
The 34.13-second control also disables the wrapper, supporting incremental
reuse as the source of the improvement rather than merely bypassing sccache.

## Recommended iteration workflow

```bash
env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=1 RUSTC_WRAPPER= \
  cargo test-clif --lib -E 'test(claude_copies::)'
```

On this 16-logical-CPU host, the same 330 default-enabled unit tests all passed
at each tested concurrency. Execution time was 15.90 seconds with two processes,
8.28 seconds with four, 5.33 seconds with eight, and 5.11 seconds with sixteen.
The sixteen-process run provided little additional benefit and individual large
streaming-parser tests took longer. These observations do not measure aggregate
peak memory or establish safe concurrency for the real-model/HTTP suite.
`-j 8` is a measured local option for this unit-only workflow; use a smaller
budget on constrained hosts. The largest remaining unit-test durations are
oversized Codex streaming-envelope checks and explicit HTTP deadline checks.
Their boundary and timeout coverage remains intact.
Two additional eight-process runs passed all 330 tests in 5.77 and 6.07 seconds.
No retries or timeout increases were used. The 55 ignored tests were not executed
by these unit-only runs; real-model and external-service checks require their
explicit environment and full gate.

Omit `-E` to run all default-enabled unit tests. The explicit `--lib` scope keeps
this workflow's purpose clear; its scope reduction alone was not faster for the
measured edit. Leave real CLI/MCP and HTTP/model contracts in the full completion
gate, and retain formatting, Clippy, code generation and doctests as appropriate.

Raw measurements, logs, and the measurement driver live outside Git under
`/home/james/scratch/mash-artifacts/test-turnaround-c1011e7`. Cargo HTML timing
reports are in the persistent target's `cargo-timings` directory.

Bead `mash-c0d.12` remains in progress. Native Mac measurements, reproducible
cold/warm repetitions, per-test critical paths, model/service setup, parallel
resource limits, and full-suite/CI optimization still require evaluation.
