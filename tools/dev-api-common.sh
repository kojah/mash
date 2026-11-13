#!/usr/bin/env bash
# Sourced by the fixed, trusted API deployment/check entry points.
set -euo pipefail
umask 077

load_api_environment() {
    local environment=$1 token_name=$2
    [[ "$token_name" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]
    set -a
    source "$environment"
    set +a
    api_token=${!token_name}
    # Curl config is passed on stdin so credentials never enter process argv.
    [[ "$api_token" =~ ^[A-Za-z0-9._~-]+$ ]]
}

api_get() {
    local base=$1 route=$2 destination=$3
    printf 'header = "Authorization: Bearer %s"\n' "$api_token" |
        curl --config - --silent --show-error --fail --connect-timeout 5 --max-time 20 \
            "$base/v1/$route" --output "$destination"
}

pointer_pid() {
    jq -er '.pid | select(type == "number" and . > 1 and floor == .)' "$1"
}

verify_api_process() {
    local pointer=$1 pid expected actual
    pid=$(pointer_pid "$pointer") || { echo 'API release record has no valid process ID.' >&2; return 1; }
    readlink "/proc/$pid/exe" >/dev/null 2>&1 || {
        echo 'Recorded API process is absent or inaccessible; reconcile the service manager and release record before deployment.' >&2
        return 1
    }
    expected=$(jq -er '.binary_sha256 | select(type == "string")' "$pointer")
    [[ "$expected" =~ ^[0-9a-f]{64}$ ]]
    actual=$(sha256sum "/proc/$pid/exe")
    [[ "${actual%% *}" == "$expected" ]] || { echo 'Running API binary does not match the release record.' >&2; return 1; }
    kill -0 "$pid" 2>/dev/null || { echo 'Deployment cannot access the recorded API process.' >&2; return 1; }
    tr '\0' '\n' < "/proc/$pid/cmdline" | grep -Fxq -- --server-config || {
        echo 'Recorded API process is not a server invocation.' >&2
        return 1
    }
}

check_api_http() {
    local base=$1 directory=$2 status
    api_get "$base" freshness "$directory/freshness.json" || return 1
    status=$(curl --silent --show-error --connect-timeout 5 --max-time 20 \
        "$base/v1/freshness" --output /dev/null --write-out '%{http_code}')
    [[ "$status" == 401 ]]
}

# Read the migration interval declared by the candidate source.
silver_schema_range() {
    local source=$1
    schema=$(sed -nE 's/^pub\(crate\) const SCHEMA_VERSION: i64 = ([0-9]+);$/\1/p' "$source")
    schema_min=$(sed -nE 's/^const BASELINE: i64 = ([0-9]+);$/\1/p' "$source")
    # Releases without migrations only open their exact schema.
    schema_min=${schema_min:-$schema}
    [[ $schema =~ ^[1-9][0-9]*$ && $schema_min =~ ^[1-9][0-9]*$ && $schema_min -le $schema ]] || {
        echo 'Cannot determine the target silver schema range; shared compatibility is unverified.' >&2
        return 1
    }
}

silver_schema_supported() {
    [[ $1 =~ ^[0-9]+$ ]] && (( $1 == 0 || ($1 >= schema_min && $1 <= schema) ))
}

# Older releases without this field support only their own exact schema.
silver_binary_range() {
    local report
    # Releases before --version-info printed this JSON from --version.
    report=$("$1" --version-info 2>/dev/null) && jq -e . >/dev/null 2>&1 <<<"$report" ||
        report=$("$1" --version) || return 1
    schema=$(jq -er '.silver_schema | select(type == "number" and . > 0 and floor == .)' <<<"$report") || return 1
    schema_min=$(jq -er '(if has("silver_schema_min") then .silver_schema_min else .silver_schema end) | select(type == "number" and . > 0 and floor == .)' <<<"$report") || return 1
    [[ $schema_min -le $schema ]]
}

# A shared silver database's schema version, read without writing to it.
silver_version() {
    python3 - "$1" <<'PYTHON'
import pathlib, sqlite3, sys
uri = pathlib.Path(sys.argv[1]).resolve().as_uri() + "?mode=ro"
print(sqlite3.connect(uri, uri=True).execute("PRAGMA user_version").fetchone()[0])
PYTHON
}

publication_identity() {
    jq -S '{nodes: [.nodes[] | {node_id, revision, last_synced_at, last_updated_at, silver_revision, source_silver}] | sort_by(.node_id)}' "$1"
}
