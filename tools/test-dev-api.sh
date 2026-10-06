#!/usr/bin/env bash
set -euo pipefail
umask 077
repository=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:?}/dev-api-test.XXXXXX")
mkdir "$scratch/bin" "$scratch/logs"
old_pid=''
cleanup() {
    if [[ -f "$scratch/pointer.json" ]]; then
        local pid
        pid=$(jq -r .pid "$scratch/pointer.json")
        if [[ "$pid" =~ ^[0-9]+$ && "$pid" -gt 1 ]]; then kill -TERM "$pid" 2>/dev/null || true; fi
    fi
    if [[ -n "$old_pid" ]]; then kill -TERM "$old_pid" 2>/dev/null || true; wait "$old_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
cat > "$scratch/serve" <<'SERVER'
printf '%s\n' "$@" > "$TEST_API_ARGUMENTS"
while :; do sleep 0.1; done
SERVER
cat > "$scratch/bin/curl" <<'CURL'
#!/usr/bin/env bash
set -euo pipefail
destination=''
authenticated=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --config) authenticated=true; cat >/dev/null; shift 2 ;;
        --output) destination=$2; shift 2 ;;
        *) shift ;;
    esac
done
if [[ "$authenticated" == false ]]; then printf 401; exit 0; fi
if [[ "${TEST_API_ALWAYS_READY:-0}" != 1 ]]; then
    pid=$(jq -r .pid "$TEST_API_POINTER")
    [[ -e "/proc/$pid" && "$(ps -p "$pid" -o stat=)" != Z* ]] || exit 22
fi
printf '%s\n' '{"nodes":[{"node_id":"fixture","revision":7}]}' > "$destination"
CURL
chmod 755 "$scratch/bin/curl"
printf '%s\n' 'TEST_API_TOKEN=test-only-token' > "$scratch/environment"
export TEST_API_POINTER="$scratch/pointer.json"
export TEST_API_ARGUMENTS="$scratch/arguments"
export PATH="$scratch/bin:$PATH"
cd "$scratch"
bash serve --server-config fixture.toml &
old_pid=$!
for ((attempt=0; attempt<50; attempt++)); do
    if [[ -e "/proc/$old_pid/exe" ]]; then break; fi
    sleep 0.1
done
executable=$(readlink "/proc/$old_pid/exe")
digest=$(sha256sum "$executable")
jq -n --argjson pid "$old_pid" --arg binary_sha256 "${digest%% *}" \
    '{pid:$pid,binary_sha256:$binary_sha256,source_commit:"fixture"}' > "$TEST_API_POINTER"
bash "$repository/tools/check-dev-api.sh" "$scratch/environment" "$TEST_API_POINTER" \
    http://fixture TEST_API_TOKEN "$scratch/logs" >/dev/null
if bash "$repository/tools/restart-dev-api.sh" /usr/bin/false fixture.toml "$scratch/environment" \
    "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs"; then
    echo 'Expected failed candidate to roll back.' >&2
    exit 1
fi
new_pid=$(jq -r .pid "$TEST_API_POINTER")
[[ "$new_pid" != "$old_pid" ]]
[[ "$(jq -r .source_commit "$TEST_API_POINTER")" == fixture ]]
[[ "$(ps -p "$new_pid" -o sid=)" != "$(ps -p $$ -o sid=)" ]]
bash "$repository/tools/check-dev-api.sh" "$scratch/environment" "$TEST_API_POINTER" \
    http://fixture TEST_API_TOKEN "$scratch/logs" >/dev/null
export TEST_API_ALWAYS_READY=1
previous_pid=$new_pid
bash "$repository/tools/restart-dev-api.sh" /usr/bin/bash fixture.toml "$scratch/environment" \
    "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" --activate-query >/dev/null
rg -qx -- --activate-query "$TEST_API_ARGUMENTS"
[[ "$(jq -r .pid "$TEST_API_POINTER")" != "$previous_pid" ]]
[[ "$(jq -r .previous_pid "$TEST_API_POINTER")" == "$previous_pid" ]]
[[ "$(jq -r .source_commit "$TEST_API_POINTER")" == "$(git -C "$repository" rev-parse HEAD)" ]]
[[ "$(ps -p "$(jq -r .pid "$TEST_API_POINTER")" -o sid=)" != "$(ps -p $$ -o sid=)" ]]
bash "$repository/tools/check-dev-api.sh" "$scratch/environment" "$TEST_API_POINTER" \
    http://fixture TEST_API_TOKEN "$scratch/logs" >/dev/null
