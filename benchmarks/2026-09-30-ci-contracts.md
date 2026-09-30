# Real-model CI contract validation

The previous workflow selected PostgreSQL/Redis tests without providing the
model directory required by four Claude-copy reconciliation cases. Standalone
real embedding, reranking, hybrid-search and parallel-retrieval contracts were
also absent from its selected ignored-test gates.

The workflow now explicitly installs models on a cache miss, verifies them with
real inference contracts, and supplies the same installed assets to both shared
backend selections. Cache keys include platform, Cargo.lock, model definitions
and installation implementation. Cache saving requires successful validation.
Each selection uses `--no-tests fail` and two test processes. Default unit tests
remain separate; benchmark-only and native Mac diagnostics remain excluded.

## Local evidence on Xenia

- Actionlint 1.7.12 accepted the modified workflow.
- An intentionally empty Nextest selection exited 4.
- An explicit installation into a fresh isolated directory succeeded: 32 files,
  384 embedding dimensions, approximately 169 MB of assets. Production model
  directories were not modified.
- The combined six-model/31-backend selection passed all 37 contracts with the
  older installed assets.
- The same selection initially failed one contract with fresh generation-backed
  assets: the embedding batch test passed the installation root directly to
  low-level loaders. It now uses `Models::load` and `Models::load_indexer`, which
  select and lease activated assets as production does.
- After that fix, all 37 selected tests passed against the fresh installation
  in 45.309 seconds of test execution. This is not a performance comparison.
- Local execution used Cranelift, the CI Nextest profile, and disposable Redis
  port 57379 and PostgreSQL/pgvector port 57432. It does not establish hosted
  LLVM compilation or native Mac behavior.

Raw installation, fresh-contract and empty-selection logs are outside Git at
`/home/james/scratch/mash-artifacts/ci-contracts-2ba4629`.

## Hosted gate still unverified

[GitHub run 36780070774](https://github.com/kojah/mash/actions/runs/36780070774)
did not start its Rust job. GitHub reported failed recent account payments or
a spending limit requiring an increase; deployment was consequently skipped.
Local validation does not replace a successful hosted run and deployment of
the exact commit. Bead `mash-c0d.2.2` stays in progress for that evidence.

Source inspection also identified direct-root assumptions in a native Core ML
benchmark; `mash-c0d.2.3` tracks reproduction and validation on Apple Silicon.
This Linux change does not claim that native benchmark is fixed.
