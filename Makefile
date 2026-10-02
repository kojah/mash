# Regenerate both query layers before Cargo resolves and compiles dependencies.
# Example: make build CODEGEN=1 CARGO_ARGS='--release --locked'
CODEGEN ?= 0
CARGO_ARGS ?=
COVERAGE_OUTPUT ?= target/llvm-cov/coverage.json

.PHONY: build generate-queries check-generated coverage
build:
ifeq ($(CODEGEN),1)
	$(MAKE) generate-queries
endif
	cargo build-clif $(CARGO_ARGS)

generate-queries:
	bash queries/generate.sh

check-generated: generate-queries
	git diff --exit-code -- generated/postgres app/storage/generated

# Coverage includes the installed-model and disposable shared-backend contracts.
# LLVM instrumentation is required here; ordinary development uses Cranelift.
coverage:
	@test -n "$${MASH_TEST_MODELS_DIR}" || (echo 'Set MASH_TEST_MODELS_DIR to explicitly installed test models'; exit 1)
	@test -n "$${MASH_TEST_PG_URL}" || (echo 'Set MASH_TEST_PG_URL to disposable PostgreSQL with pgvector'; exit 1)
	@test -n "$${MASH_TEST_REDIS_URL}" || (echo 'Set MASH_TEST_REDIS_URL to disposable Redis Search'; exit 1)
	@test -n "$${MASH_TEST_QDRANT_URL}" || (echo 'Set MASH_TEST_QDRANT_URL to disposable Qdrant 1.19+'; exit 1)
	@test -n "$${MASH_TEST_SERVER_BINARY}" || (echo 'Set MASH_TEST_SERVER_BINARY to the coverage-target mash binary for database restart contracts'; exit 1)
	# Persistent targets can retain old binaries and mix obsolete source mappings.
	python3 tools/wait-coverage-workers.py
	cargo llvm-cov clean --workspace
	cargo llvm-cov nextest --no-report --run-ignored all -j 2 -E 'not test(benchmark)' $(CARGO_ARGS)
	python3 tools/wait-coverage-workers.py
	cargo llvm-cov report --json --ignore-filename-regex '/generated/|/(tests|benchmarks)(/|\.rs$$)|/topics(/|\.rs$$)|/test_support\.rs$$|/semantic/evaluation(/|\.rs$$)|/semantic/embeddings/cpu/sensitivity\.rs$$|/storage/indexing/memory\.rs$$' --output-path "$(COVERAGE_OUTPUT)"
