#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Keep the checked-in output reproducible, including generator/runtime versions.
test "$(cornucopia --version)" = "cornucopia 1.0.1"
test "$(sqlc version)" = "v1.31.1"
if [[ -n "${MASH_CODEGEN_POSTGRES_URL:-}" ]]; then
    # Creates and drops a temporary database; never validates against live data.
    cornucopia fresh queries/postgres/_extensions.sql app/shared/migrations.sql app/shared/schema.sql \
        --url "$MASH_CODEGEN_POSTGRES_URL"
else
    cornucopia schema queries/postgres/_extensions.sql app/shared/migrations.sql app/shared/schema.sql \
        --container-image pgvector/pgvector:pg17
fi
sqlc generate
rustfmt --edition 2024 generated/postgres/src/lib.rs app/storage/generated/queries.rs
