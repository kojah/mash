#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Keep checked-in SQLite queries reproducible.
test "$(sqlc version)" = "v1.31.1"
sqlc generate
rustfmt --edition 2024 crates/mash-storage/src/storage/generated/queries.rs crates/mash-sync/src/shared/silver_store/generated/queries.rs
