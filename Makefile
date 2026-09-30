# Regenerate both query layers before Cargo resolves and compiles dependencies.
# Example: make build CODEGEN=1 CARGO_ARGS='--release --locked'
CODEGEN ?= 0
CARGO_ARGS ?=

.PHONY: build generate-queries check-generated
build:
ifeq ($(CODEGEN),1)
	$(MAKE) generate-queries
endif
	cargo build-clif $(CARGO_ARGS)

generate-queries:
	bash queries/generate.sh

check-generated: generate-queries
	git diff --exit-code -- generated/postgres app/storage/generated
