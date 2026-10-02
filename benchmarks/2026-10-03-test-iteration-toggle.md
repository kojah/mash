# Incremental test iteration after completion builds

Tracking: `mash-c0d.12.2`, within the still-open test-turnaround work.
Base source: `d21c91d6a802986b213f1f20661c88e994456c93` on Linux/Xenia.
This bounded control checks whether the required nonincremental completion
workflow disrupts the warmed incremental library-test workflow. It does not
change compiler settings, test coverage, dependencies or runtime behavior.

## Procedure and measurements

The same persistent integrated target and temporary directories were used at
every stage. Dependencies were already built. `RUSTC_WRAPPER=` bypassed the
host's configured sccache; Cranelift compiled mash through `cargo test-clif`.
Build-only stages used `--no-run --lib --timings`. A temporary change to one
assertion's diagnostic string in the Gemini tool-history tests changed no test
condition or fixture. All original source bytes were restored afterward.

| Stage | Incremental | Wall time, seconds | Scope |
| --- | --- | ---: | --- |
| Warm current library-test build | On | 20.65 | Build only |
| Diagnostic-string edit | On | 11.60 | Build only |
| Same edited source, completion setting | Off | 56.04 | Build only |
| Same source, return to iteration setting | On | 1.13 | Build only; Cargo reused completed artifacts |
| Restore exact original source | On | 13.16 | Build plus two focused tests |

The restored-source tests passed in 0.091s. The 13.16s total includes compiling,
nextest startup and execution; it is not a test-execution-only observation.
Cargo's HTML report assigns 55.21s of the nonincremental build to the single
`mash` library-test unit, versus 55.53s reported by Cargo for that build. It does
not separate type checking, code generation and linking within the unit
(`sections` was null); no narrower compiler-bottleneck attribution follows.

The 22 recorded mash incremental dependency-graph paths, sizes and modification
times were identical immediately before and after the nonincremental build.
The fast return to incremental mode supports reuse in this control. File
metadata alone is not a full audit of every cache byte or compiler query.
The hypothesis that completion builds erase the warmed cache was not supported;
do not add a separate target directory to remedy that presumed failure.

These are one ordered development control, not repeated p50/p95 measurements.
Background host load was uncontrolled. The experiment does not measure a cold
checkout, new public types/functions/macros, dependency changes, native Mac
builds, inference, backend startup, CLI process startup or hosted CI. It cannot
explain every earlier minute-long rebuild or promise 13s for all edits.

## Workflow decision

Keep the existing focused iteration command:

```bash
env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=1 RUSTC_WRAPPER= \
  cargo test-clif --lib -E 'test(ingestion::gemini::tests::tool_history::)'
```

Retain all required nonincremental completion, CLI/MCP, model/backend, lint,
formatting, code-generation and doctest checks. Filtering tests does not divide
the compiler's work inside the library-test unit. A future crate-boundary or
test-binary change should measure edit/build paths and retain the full integration
gate; this result alone does not establish the benefit of such a refactor.

Raw logs, `/usr/bin/time` elapsed/CPU files, original-source bytes, cache
metadata, parsed Cargo unit timings, toolchain details and `results.json` are in
`/home/james/scratch/mash-artifacts/test-iteration-toggle-20261003`. Cargo HTML
timings remain in the integrated target's `cargo-timings` directory. The original
source SHA-256 is
`85b16a0e3ca22fec2a3083c8f4082068f6d00f3affe5b27c21402cd39921ec29`.

## Current hosted gate

For the exact base commit, [CI run 37073471381](https://github.com/kojah/mash/actions/runs/37073471381)
completed with no Rust steps executed. The current Rust check annotation reports
an account payment/spending-limit problem; deployment was skipped with no steps.
The jobs and annotations were freshly read from GitHub's API and retained in
this artifact directory. Local measurements and passing tests do not replace
hosted CI or exact-commit fleet deployment. Beads `mash-c0d.2.2` and
`mash-c0d.8.2` retain those open gates; account intervention is still required.
