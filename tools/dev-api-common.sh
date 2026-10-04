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
    local pointer=$1 pid executable expected actual
    pid=$(pointer_pid "$pointer")
    executable=$(readlink "/proc/$pid/exe")
    expected=$(jq -er '.binary_sha256 | select(type == "string")' "$pointer")
    [[ "$expected" =~ ^[0-9a-f]{64}$ ]]
    actual=$(sha256sum "$executable")
    [[ "${actual%% *}" == "$expected" ]]
    tr '\0' '\n' < "/proc/$pid/cmdline" | grep -Fxq serve
}

check_api_http() {
    local base=$1 directory=$2 status
    api_get "$base" freshness "$directory/freshness.json" || return 1
    api_get "$base" capabilities "$directory/capabilities.json" || return 1
    status=$(curl --silent --show-error --connect-timeout 5 --max-time 20 \
        "$base/v1/freshness" --output /dev/null --write-out '%{http_code}')
    [[ "$status" == 401 ]]
}

publication_identity() {
    jq -S '{nodes: [.nodes[] | {node_id, revision, last_synced_at, last_updated_at, silver_revision, source_silver}] | sort_by(.node_id)}' "$1"
}
