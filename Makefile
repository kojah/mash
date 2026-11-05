# Regenerate SQLite queries before Cargo resolves and compiles dependencies.
# Example: make build CODEGEN=1 CARGO_ARGS='--release --locked'
CODEGEN ?= 0
CARGO_ARGS ?=
COVERAGE_OUTPUT ?= target/llvm-cov/coverage.json
DEV_TMPDIR ?= /home/james/scratch/achi-tmp
DEV_TARGET_DIR ?= /home/james/scratch/achi-targets/integrated
DEV_RUN ?= $(if $(wildcard /home/james/.local/bin/agent-test),/home/james/.local/bin/agent-test,)
export FOCUS

.PHONY: mutation-tools mutation-check mutation-connectors mutation-certification property-check property-backend-check
mutation-tools:
	bash tools/mutation-check.sh --install-tools

mutation-check:
	$(DEV_RUN) bash tools/mutation-check.sh

mutation-connectors:
	$(DEV_RUN) bash tools/mutation-check.sh --connectors

mutation-certification:
	$(DEV_RUN) bash tools/mutation-check.sh --certification

.PHONY: mutation-runtime
mutation-runtime:
	$(DEV_RUN) bash tools/mutation-check.sh --runtime

property-check:
	$(MAKE) iterate FOCUS='test(connectors::silver::tests::properties)'

property-backend-check:
	@test -n "$${MASH_TEST_QDRANT_URL}" || (echo 'Set MASH_TEST_QDRANT_URL to disposable Qdrant 1.19+'; exit 2)
	$(DEV_RUN) env TMPDIR="$(DEV_TMPDIR)" CARGO_TARGET_DIR="$(DEV_TARGET_DIR)" CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --locked --lib --run-ignored only -E 'test(connector_sequences)' --no-tests fail --test-threads 1

.PHONY: protocol-tools protocol-check
protocol-tools:
	bash protocol/check.sh --install-tools

protocol-check:
	$(DEV_RUN) bash protocol/check.sh

.PHONY: iterate validate-dev
iterate:
	@test -n "$${FOCUS}" || (echo 'Set FOCUS to a nextest filter for the changed behavior'; exit 2)
	$(DEV_RUN) env TMPDIR="$(DEV_TMPDIR)" CARGO_TARGET_DIR="$(DEV_TARGET_DIR)" CARGO_INCREMENTAL=1 RUSTC_WRAPPER= bash tools/build-dev-linux.sh --focused "$${FOCUS}"

validate-dev:
	$(DEV_RUN) env TMPDIR="$(DEV_TMPDIR)" CARGO_TARGET_DIR="$(DEV_TARGET_DIR)" CARGO_INCREMENTAL=0 bash tools/build-dev-linux.sh --validate-only

.PHONY: build check-skills generate-skills generate-queries check-generated coverage generate-python-models check-python-models
build: check-skills
ifeq ($(CODEGEN),1)
	$(MAKE) generate-queries
endif
	cargo build-clif $(CARGO_ARGS)

check-skills:
	bash tools/generate-history-skill.sh --check
	cargo test-clif --locked --lib -E 'test(history_skill_)' --no-tests fail

generate-skills:
	bash tools/generate-history-skill.sh

generate-queries:
	bash queries/generate.sh

check-generated: generate-queries
	git diff --exit-code -- app/storage/generated app/shared/silver_store/generated

generate-python-models:
	bash tools/generate-python-models.sh

check-python-models: generate-python-models
	git ls-files --error-unmatch sdk/python/src/mash_sdk/_generated/batch.py sdk/python/src/mash_sdk/_generated/receipt.py sdk/python/src/mash_sdk/_generated/batch.schema.json sdk/python/src/mash_sdk/_generated/receipt.schema.json >/dev/null
	git diff --exit-code -- sdk/python/src/mash_sdk/_generated

# Coverage includes the installed-model and disposable shared-backend contracts.
# LLVM instrumentation is required here; ordinary development uses Cranelift.
coverage:
	@test -n "$${MASH_TEST_MODELS_DIR}" || (echo 'Set MASH_TEST_MODELS_DIR to explicitly installed test models'; exit 1)
	@test -n "$${MASH_TEST_QDRANT_URL}" || (echo 'Set MASH_TEST_QDRANT_URL to disposable Qdrant 1.19+'; exit 1)
	@test -n "$${MASH_TEST_SERVER_BINARY}" || (echo 'Set MASH_TEST_SERVER_BINARY to the coverage-target mash binary for database restart contracts'; exit 1)
	cargo llvm-cov clean --workspace
	cargo llvm-cov nextest --no-report --run-ignored all -j 2 -E 'not test(benchmark)' $(CARGO_ARGS)
	cargo llvm-cov report --json --ignore-filename-regex '/generated/|/(tests|benchmarks)(/|\.rs$$)|/test_support\.rs$$|/semantic/evaluation(/|\.rs$$)|/semantic/embeddings/cpu/sensitivity\.rs$$|/storage/indexing/memory\.rs$$' --output-path "$(COVERAGE_OUTPUT)"
