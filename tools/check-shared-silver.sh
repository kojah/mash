#!/usr/bin/env bash
# Read-only prerequisite for installers that preserve the existing shared root.
# Cross-schema retirement/republish is a separate, explicitly planned operation.
set -euo pipefail
[[ $# == 2 && -f $1 && -d $2 ]] || {
    echo 'usage: check-shared-silver.sh SILVER_SCHEMA_SOURCE SHARED_SILVER_DIRECTORY' >&2
    exit 2
}
source "$(dirname "$0")/dev-api-common.sh"
schema=$(sed -nE 's/^pub\(crate\) const SCHEMA_VERSION: i64 = ([0-9]+);$/\1/p' "$1")
[[ $schema =~ ^[1-9][0-9]*$ ]] || {
    echo 'Cannot determine the target silver schema; shared compatibility is unverified.' >&2
    exit 1
}
shopt -s nullglob
incompatible=false
for directory in "$2" "$2"/*; do
    [[ -d $directory ]] || continue
    [[ -r $directory && -x $directory ]] || {
        echo 'Cannot inspect the shared silver directory; schema compatibility is unverified.' >&2
        exit 1
    }
done
for file in "$2"/*/*.sqlite3; do
    version=$(silver_version "$file")
    if [[ $version != 0 && $version != "$schema" ]]; then incompatible=true; fi
done
if [[ $incompatible == true ]]; then
    echo "Shared silver is incompatible with target schema $schema. Prepare a schema-aware rollout preserving node history and transfer receipts before deployment; no deployment performed." >&2
    exit 1
fi
printf 'Shared silver is compatible with target schema %s; read-only inspection only.\n' "$schema"
