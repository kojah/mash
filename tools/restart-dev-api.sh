#!/usr/bin/env bash
set -euo pipefail
usage() {
    echo 'usage: restart-dev-api.sh ARTIFACT CONFIG ENV POINTER API_URL TOKEN_ENV LOG_ROOT [--activate-query] [--replay-views] [--rollback-config PATH] [--retire-incompatible-silver SILVER_DIR]' >&2
    exit 2
}
[[ $# -ge 7 ]] || usage
source "$(dirname "$0")/dev-api-common.sh"
artifact=$1 config=$2 environment=$3 pointer=$4 base=$5 token_name=$6 root=$7
shift 7
activate_query='' replay_views='' rollback_config=$config silver_dir=''
while [[ $# -gt 0 ]]; do
    case "$1" in
        --activate-query) activate_query=--activate-query; shift ;;
        --replay-views) replay_views=--replay-views; shift ;;
        # A breaking TOML change needs the previous release's config on rollback.
        --rollback-config) [[ $# -ge 2 && -f $2 ]] || usage; rollback_config=$2; shift 2 ;;
        # Across a silver schema change the new release cannot open old shared
        # silver. Retire those node databases so their nodes republish; never
        # implied, because it discards what the nodes last published.
        --retire-incompatible-silver) [[ $# -ge 2 && $2 == /* ]] || usage; silver_dir=$2; shift 2 ;;
        *) usage ;;
    esac
done
exec {deploy_lock}>"$pointer.deploy.lock"
flock -w 60 "$deploy_lock" || { echo 'Another API rollout is active; inspect its job before retrying.' >&2; exit 1; }
load_api_environment "$environment" "$token_name"
verify_api_process "$pointer"
old_pid=$(pointer_pid "$pointer")
old_binary=$(readlink "/proc/$old_pid/exe")
mkdir -p "$root"
scratch=$(mktemp -d "$root/api-deploy.XXXXXX")
check_api_http "$base" "$scratch"
publication_identity "$scratch/freshness.json" > "$scratch/before.json"
cp "$pointer" "$scratch/previous-release.json"
digest=$(sha256sum "$artifact")
digest=${digest%% *}
config_digest=$(sha256sum "$config")
config_digest=${config_digest%% *}
environment_digest=$(sha256sum "$environment")
environment_digest=${environment_digest%% *}
# Do not interrupt healthy sessions for an identical artifact/configuration.
# Explicit replay/activation still runs even when the binary is unchanged.
if [[ -z "$activate_query$replay_views" ]] && jq -e \
    --arg binary "$digest" --arg config "$config_digest" --arg environment "$environment_digest" \
    '.binary_sha256 == $binary and .config_sha256 == $config and .environment_sha256 == $environment' "$pointer" >/dev/null; then
    compatible=true
    if [[ -n "$silver_dir" ]]; then
        schema=$("$artifact" --version | jq -er '.silver_schema | select(type == "number")')
        shopt -s nullglob
        for file in "$silver_dir"/*/*.sqlite3; do
            version=$(silver_version "$file")
            [[ "$version" == 0 || "$version" == "$schema" ]] || compatible=false
        done
    fi
    if [[ "$compatible" == true ]]; then
        jq '{source_commit,binary_sha256,pid,authenticated_readiness:true,publications_preserved:true,unchanged:true}' "$pointer"
        exit 0
    fi
fi
binary="$root/mash-$digest"
bash "$(dirname "$0")/install-dev-binary.sh" "$artifact" "$binary"
revision=$(git -C "$(dirname "$0")/.." rev-parse HEAD)
new_pid=''
committed=false
stopped=false
retired=()

start_api() {
    # Detach the API session from the short-lived deployment/MCP process group.
    # nohup alone does not protect it from process-group cleanup on disconnect.
    local flags=()
    [[ ${3:-} == --activate-query ]] && flags+=(--activate-query)
    [[ ${4:-} == --replay-views ]] && flags+=(--replay-views)
    # The long-lived server must not inherit the short-lived rollout lock.
    nohup setsid "$1" serve --server-config "${5:-$config}" "${flags[@]}" {deploy_lock}>&- </dev/null >"$2" 2>&1 &
    new_pid=$!
}

wait_exit() {
    local pid=$1
    for ((attempt=0; attempt<300; attempt++)); do
        [[ ! -e "/proc/$pid" ]] && return 0
        [[ "$(ps -p "$pid" -o stat=)" == Z* ]] && return 0
        sleep 0.1
    done
    return 1
}

write_pointer() {
    local source=$1 pid=$2 destination=$3
    jq --argjson pid "$pid" '.pid=$pid' "$source" > "$destination.incoming.$$"
    mv "$destination.incoming.$$" "$destination"
}

rollback() {
    local status=$?
    # After the old API stops, any failure restarts it, even before a new one
    # started (retiring silver can fail too).
    if [[ "$committed" != true && "$stopped" == true ]]; then
        if [[ -n "$new_pid" ]]; then
            kill -TERM "$new_pid" 2>/dev/null || true
            if ! wait_exit "$new_pid"; then
                echo 'New API did not stop; rollback not started. Inspect private logs.' >&2
                exit 1
            fi
        fi
        restore_silver
        start_api "$old_binary" "$scratch/rollback.log" '' '' "$rollback_config"
        write_pointer "$scratch/previous-release.json" "$new_pid" "$pointer"
        echo 'API deployment failed; previous binary restarted. Inspect private logs.' >&2
    fi
    exit "$status"
}
retire_silver() {
    [[ -n "$silver_dir" ]] || return 0
    local schema file version group node
    schema=$("$binary" --version | jq -er '.silver_schema | select(type == "number")')
    shopt -s nullglob
    for file in "$silver_dir"/*/*.sqlite3; do
        version=$(silver_version "$file")
        [[ "$version" == 0 || "$version" == "$schema" ]] && continue
        group=$(basename "$(dirname "$file")")
        node=$(basename "$file" .sqlite3)
        mkdir -p "$scratch/retired-silver/$group"
        for part in "$file" "$file-wal" "$file-shm"; do
            [[ -e "$part" ]] && mv "$part" "$scratch/retired-silver/$group/"
        done
        retired+=("$node")
    done
}

restore_silver() {
    [[ -d "$scratch/retired-silver" ]] || return 0
    local directory part
    for directory in "$scratch/retired-silver"/*; do
        for part in "$directory"/*; do
            mv "$part" "$silver_dir/$(basename "$directory")/"
        done
    done
}

trap rollback EXIT
kill -TERM "$old_pid"
wait_exit "$old_pid" || { echo 'Previous API did not stop; no replacement started.' >&2; exit 1; }
stopped=true
retire_silver
start_api "$binary" "$scratch/api.log" "$activate_query" "$replay_views"
ready=false
for ((attempt=0; attempt<60; attempt++)); do
    kill -0 "$new_pid" 2>/dev/null || break
    if check_api_http "$base" "$scratch" 2>"$scratch/readiness-error.log"; then ready=true; break; fi
    sleep 1
done
[[ "$ready" == true ]]
publication_identity "$scratch/freshness.json" > "$scratch/after.json"
# Nodes whose silver was retired republish; every other node keeps its publication.
retired_json=$(printf '%s\n' "${retired[@]}" | jq -R . | jq -sc 'map(select(length > 0))')
for side in before after; do
    jq --argjson retired "$retired_json" '.nodes |= map(select(.node_id as $n | $retired | index($n) | not))' \
        "$scratch/$side.json" > "$scratch/$side.kept.json"
done
cmp "$scratch/before.kept.json" "$scratch/after.kept.json"
jq -n --arg source_commit "$revision" --arg binary_sha256 "$digest" \
    --arg config_sha256 "$config_digest" --arg environment_sha256 "$environment_digest" \
    --arg log "$scratch/api.log" --argjson pid "$new_pid" --argjson previous_pid "$old_pid" \
    --argjson retired "$retired_json" --arg retired_dir "$scratch/retired-silver" \
    '{source_commit:$source_commit, binary_sha256:$binary_sha256, config_sha256:$config_sha256, environment_sha256:$environment_sha256, log:$log, pid:$pid, previous_pid:$previous_pid, build:"optimized LLVM release", deployed:true}
     + if ($retired | length) > 0 then {republish:$retired, retired_silver:$retired_dir} else {} end' \
    > "$pointer.incoming.$$"
mv "$pointer.incoming.$$" "$pointer"
committed=true
jq '{source_commit,binary_sha256,pid,authenticated_readiness:true,publications_preserved:((.republish // []) | length == 0),republish:(.republish // [])}' "$pointer"
