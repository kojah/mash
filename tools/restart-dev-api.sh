#!/usr/bin/env bash
set -euo pipefail
[[ $# == 7 ]] || { echo 'usage: restart-dev-api.sh ARTIFACT CONFIG ENV POINTER API_URL TOKEN_ENV LOG_ROOT' >&2; exit 2; }
source "$(dirname "$0")/dev-api-common.sh"
artifact=$1 config=$2 environment=$3 pointer=$4 base=$5 token_name=$6 root=$7
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
binary="$root/mash-$digest"
bash "$(dirname "$0")/install-dev-binary.sh" "$artifact" "$binary"
revision=$(git -C "$(dirname "$0")/.." rev-parse HEAD)
new_pid=''
committed=false

start_api() {
    nohup "$1" serve --server-config "$config" </dev/null >"$2" 2>&1 &
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
    if [[ "$committed" != true && -n "$new_pid" ]]; then
        kill -TERM "$new_pid" 2>/dev/null || true
        if ! wait_exit "$new_pid"; then
            echo 'New API did not stop; rollback not started. Inspect private logs.' >&2
            exit 1
        fi
        start_api "$old_binary" "$scratch/rollback.log"
        write_pointer "$scratch/previous-release.json" "$new_pid" "$pointer"
        echo 'API deployment failed; previous binary restarted. Inspect private logs.' >&2
    fi
    exit "$status"
}
trap rollback EXIT
kill -TERM "$old_pid"
wait_exit "$old_pid" || { echo 'Previous API did not stop; no replacement started.' >&2; exit 1; }
start_api "$binary" "$scratch/api.log"
ready=false
for ((attempt=0; attempt<60; attempt++)); do
    kill -0 "$new_pid" 2>/dev/null || break
    if check_api_http "$base" "$scratch" 2>"$scratch/readiness-error.log"; then ready=true; break; fi
    sleep 1
done
[[ "$ready" == true ]]
publication_identity "$scratch/freshness.json" > "$scratch/after.json"
cmp "$scratch/before.json" "$scratch/after.json"
jq -n --arg source_commit "$revision" --arg binary_sha256 "$digest" \
    --arg log "$scratch/api.log" --argjson pid "$new_pid" --argjson previous_pid "$old_pid" \
    '{source_commit:$source_commit, binary_sha256:$binary_sha256, log:$log, pid:$pid, previous_pid:$previous_pid, build:"optimized LLVM release", deployed:true}' \
    > "$pointer.incoming.$$"
mv "$pointer.incoming.$$" "$pointer"
committed=true
jq '{source_commit,binary_sha256,pid,authenticated_readiness:true,publications_preserved:true}' "$pointer"
