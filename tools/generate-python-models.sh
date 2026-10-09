#!/usr/bin/env bash
# Rust owns the wire schema; Python models and bundled schemas are derived artifacts.
set -euo pipefail
repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
sdk_dir="$repo_dir/sdk/python"
generated_dir="$sdk_dir/src/mash_sdk/_generated"
mkdir -p "$repo_dir/target"
scratch_dir=$(mktemp -d "$repo_dir/target/connector-codegen.XXXXXXXX")
trap 'rm -rf "$scratch_dir"' EXIT
cd "$repo_dir"
schema_cargo_run=${MASH_SCHEMA_CARGO_RUN:-run-clif}
case "$schema_cargo_run" in
    run|run-clif) ;;
    *) echo 'MASH_SCHEMA_CARGO_RUN must be run or run-clif' >&2; exit 2 ;;
esac

for kind in batch receipt; do
    cargo "$schema_cargo_run" --locked --example connector-schema -- "$kind" > "$scratch_dir/$kind.schema.json"
    uv run --project "$sdk_dir" --locked datamodel-codegen \
        --input "$scratch_dir/$kind.schema.json" --input-file-type jsonschema \
        --output "$scratch_dir/$kind.py" --output-model-type pydantic_v2.BaseModel \
        --target-python-version 3.11 --disable-timestamp --formatters builtin \
        --use-annotated --collapse-root-models --strict-nullable \
        --infer-union-variant-names --use-one-literal-as-default \
        --strict-types str int bool \
        --type-overrides '{"Metadata":"mash_sdk._types.Metadata","ConnectorId":"builtins.str","ConnectorKind":"builtins.str"}'
    uv run --project "$sdk_dir" --locked ruff format "$scratch_dir/$kind.py"
done

mkdir -p "$generated_dir"
for kind in batch receipt; do
    cp "$scratch_dir/$kind.schema.json" "$generated_dir/$kind.schema.json"
    cp "$scratch_dir/$kind.py" "$generated_dir/$kind.py"
done
