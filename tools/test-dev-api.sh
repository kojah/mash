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
cat > "$scratch/server.rs" <<'SERVER'
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--version") {
        println!("{{\"version\":\"test\",\"silver_schema\":51}}");
        return;
    }
    std::fs::write(std::env::var("TEST_API_ARGUMENTS").unwrap(), args.join("\n") + "\n").unwrap();
    if let Ok(path) = std::env::var("TEST_API_FAILED_START_WRITE") {
        std::fs::write(path, "candidate changed durable state").unwrap();
        std::process::exit(1);
    }
    loop { std::thread::sleep(std::time::Duration::from_millis(100)); }
}
SERVER
rustc "$scratch/server.rs" -o "$scratch/server"
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
printf '# test-only server configuration\n' > fixture.toml
"$scratch/server" --server-config fixture.toml &
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
bash "$repository/tools/check-dev-api.sh" --deployment "$scratch/environment" "$TEST_API_POINTER" \
    http://fixture TEST_API_TOKEN "$scratch/logs" >/dev/null
mkdir "$scratch/empty-silver"
if bash "$repository/tools/restart-dev-api.sh" /usr/bin/false fixture.toml "$scratch/environment" \
    "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" --rollback-config "$scratch/environment" \
    --retire-incompatible-silver "$scratch/empty-silver"; then
    echo 'Expected failed candidate to roll back.' >&2
    exit 1
fi
new_pid=$(jq -r .pid "$TEST_API_POINTER")
[[ "$new_pid" != "$old_pid" ]]
[[ "$(jq -r .source_commit "$TEST_API_POINTER")" == fixture ]]
grep -qxF -- "$scratch/environment" "$TEST_API_ARGUMENTS"
[[ "$(ps -p "$new_pid" -o sid=)" != "$(ps -p $$ -o sid=)" ]]
bash "$repository/tools/check-dev-api.sh" "$scratch/environment" "$TEST_API_POINTER" \
    http://fixture TEST_API_TOKEN "$scratch/logs" >/dev/null
export TEST_API_ALWAYS_READY=1
previous_pid=$new_pid
bash "$repository/tools/restart-dev-api.sh" "$scratch/server" fixture.toml "$scratch/environment" \
    "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" --activate-query --replay-views >/dev/null
grep -qxF -- --activate-query "$TEST_API_ARGUMENTS"
grep -qxF -- --replay-views "$TEST_API_ARGUMENTS"
[[ "$(jq -r .pid "$TEST_API_POINTER")" != "$previous_pid" ]]
[[ "$(jq -r .previous_pid "$TEST_API_POINTER")" == "$previous_pid" ]]
[[ "$(jq -r .source_commit "$TEST_API_POINTER")" == "$(git -C "$repository" rev-parse HEAD)" ]]
[[ "$(ps -p "$(jq -r .pid "$TEST_API_POINTER")" -o sid=)" != "$(ps -p $$ -o sid=)" ]]
bash "$repository/tools/check-dev-api.sh" "$scratch/environment" "$TEST_API_POINTER" \
    http://fixture TEST_API_TOKEN "$scratch/logs" >/dev/null
# A failed start may already have changed durable state. Keep the recovery
# marker and never restart the previous process or discard the candidate write.
previous_pid=$(jq -r .pid "$TEST_API_POINTER")
export TEST_API_FAILED_START_WRITE="$scratch/candidate-write"
export TEST_API_ALWAYS_READY=0
if bash "$repository/tools/restart-dev-api.sh" "$scratch/server" fixture.toml \
    "$scratch/environment" "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" \
    --activate-query 2> "$scratch/activation-error"; then
    echo 'Failed candidate activation reported success.' >&2; exit 1
fi
grep -Fq 'Recover forward; no automatic rollback' "$scratch/activation-error"
[[ $(jq -r .pid "$TEST_API_POINTER") == "$previous_pid" ]]
[[ ! -e /proc/$previous_pid || $(ps -p "$previous_pid" -o stat=) == Z* ]]
[[ -f "$TEST_API_POINTER.deploy-pending" ]]
grep -Fxq 'candidate changed durable state' "$scratch/candidate-write"
if bash "$repository/tools/restart-dev-api.sh" "$scratch/server" fixture.toml \
    "$scratch/environment" "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" \
    2> "$scratch/pending-error"; then
    echo 'Pending activation was ignored.' >&2; exit 1
fi
grep -Fq 'Unfinished API rollout' "$scratch/pending-error"
# Explicit fixture recovery: no actual database was migrated by the stand-in.
unset TEST_API_FAILED_START_WRITE
"$scratch/server" --server-config fixture.toml &
recovery_pid=$!
jq --argjson pid "$recovery_pid" '.pid=$pid' "$TEST_API_POINTER" > "$scratch/recovered.json"
mv "$scratch/recovered.json" "$TEST_API_POINTER"
rm "$TEST_API_POINTER.deploy-pending"
export TEST_API_ALWAYS_READY=1
# Across a silver schema change, opt-in retirement moves incompatible shared
# silver aside so its node republishes, and keeps compatible silver in place.
# An identical healthy rollout keeps the process; configuration changes restart it.
previous_pid=$(jq -r .pid "$TEST_API_POINTER")
report=$(bash "$repository/tools/restart-dev-api.sh" "$scratch/server" fixture.toml \
    "$scratch/environment" "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs")
[[ $(jq -r .unchanged <<<"$report") == true ]]
[[ $(jq -r .pid "$TEST_API_POINTER") == "$previous_pid" ]]
printf '# changed configuration\n' >> fixture.toml
bash "$repository/tools/restart-dev-api.sh" "$scratch/server" fixture.toml \
    "$scratch/environment" "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" >/dev/null
[[ $(jq -r .pid "$TEST_API_POINTER") != "$previous_pid" ]]
silver="$scratch/silver"
mkdir -p "$silver/group"
python3 - "$silver/group" <<'PYTHON'
import sqlite3, sys
for name, version in (("old", 46), ("current", 51), ("migratable", 50)):
    connection = sqlite3.connect(f"{sys.argv[1]}/{name}.sqlite3")
    connection.execute(f"PRAGMA user_version = {version}")
    connection.commit()
PYTHON
cat > "$scratch/release" <<'RELEASE'
#!/usr/bin/env bash
if [[ ${1:-} == --version ]]; then echo '{"version":"test","silver_schema":51,"silver_schema_min":49}'; exit 0; fi
printf '%s\n' "$@" > "$TEST_API_ARGUMENTS"
while :; do sleep 0.1; done
RELEASE
cat > "$scratch/broken-release" <<'RELEASE'
#!/usr/bin/env bash
echo 'not a schema report'
RELEASE
chmod 755 "$scratch/release" "$scratch/broken-release"
# A same-root installer must discover incompatible silver during read-only
# preflight, before stopping the service or changing any database files.
printf 'pub(crate) const SCHEMA_VERSION: i64 = 51;\n' > "$scratch/schema.rs"
sha256sum "$silver/group/old.sqlite3" "$silver/group/current.sqlite3" > "$scratch/silver-before.sha256"
previous_pid=$(jq -r .pid "$TEST_API_POINTER")
if bash "$repository/tools/check-shared-silver.sh" "$scratch/schema.rs" "$silver" \
    > "$scratch/schema-output" 2> "$scratch/schema-error"; then
    echo 'Incompatible shared silver passed preflight.' >&2; exit 1
fi
grep -Fq 'preserving node history and transfer receipts' "$scratch/schema-error"
sha256sum --check "$scratch/silver-before.sha256" >/dev/null
[[ $(jq -r .pid "$TEST_API_POINTER") == "$previous_pid" ]]
kill -0 "$previous_pid"
mkdir -p "$scratch/compatible-silver/group"
cp "$silver/group/current.sqlite3" "$scratch/compatible-silver/group/current.sqlite3"
bash "$repository/tools/check-shared-silver.sh" "$scratch/schema.rs" "$scratch/compatible-silver" >/dev/null
# Supported migration versions pass without modifying history; versions on
# either side of the interval fail. Schema zero remains a new database.
printf 'const BASELINE: i64 = 49;\npub(crate) const SCHEMA_VERSION: i64 = 51;\n' > "$scratch/migrating-schema.rs"
for version in 0 48 49 50 51 52; do
    python3 - "$scratch/compatible-silver/group/current.sqlite3" "$version" <<'PY'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    db.execute(f'PRAGMA user_version={int(sys.argv[2])}')
PY
    sha256sum "$scratch/compatible-silver/group/current.sqlite3" > "$scratch/migration-before.sha256"
    if bash "$repository/tools/check-shared-silver.sh" "$scratch/migrating-schema.rs" "$scratch/compatible-silver" >/dev/null 2>&1; then
        [[ $version == 0 || ( $version -ge 49 && $version -le 51 ) ]]
    else
        [[ $version == 48 || $version == 52 ]]
    fi
    sha256sum --check "$scratch/migration-before.sha256" >/dev/null
done
cp "$silver/group/current.sqlite3" "$scratch/compatible-silver/group/current.sqlite3"
printf 'const BASELINE: i64 = 52;\npub(crate) const SCHEMA_VERSION: i64 = 51;\n' > "$scratch/migrating-schema.rs"
if bash "$repository/tools/check-shared-silver.sh" "$scratch/migrating-schema.rs" "$scratch/compatible-silver" 2>/dev/null; then
    echo 'Invalid migration interval passed preflight.' >&2; exit 1
fi
printf 'not a schema declaration\n' > "$scratch/invalid-schema.rs"
if bash "$repository/tools/check-shared-silver.sh" "$scratch/invalid-schema.rs" "$silver" 2>/dev/null; then
    echo 'Unverified target schema passed preflight.' >&2; exit 1
fi
printf 'not a database\n' > "$scratch/compatible-silver/group/broken.sqlite3"
if bash "$repository/tools/check-shared-silver.sh" "$scratch/schema.rs" "$scratch/compatible-silver" 2>/dev/null; then
    echo 'Unreadable shared schema passed preflight.' >&2; exit 1
fi
rm "$scratch/compatible-silver/group/broken.sqlite3"
chmod 000 "$scratch/compatible-silver/group"
# Root can still inspect mode-000 directories; exercise rejection only when the
# current test identity actually lacks the required access.
if [[ ! -r "$scratch/compatible-silver/group" || ! -x "$scratch/compatible-silver/group" ]]; then
    if bash "$repository/tools/check-shared-silver.sh" "$scratch/schema.rs" "$scratch/compatible-silver" \
        > "$scratch/schema-output" 2> "$scratch/schema-error"; then
        chmod 700 "$scratch/compatible-silver/group"
        echo 'Uninspectable shared directory passed preflight.' >&2; exit 1
    fi
    grep -Fq 'Cannot inspect the shared silver directory' "$scratch/schema-error"
fi
chmod 700 "$scratch/compatible-silver/group"
# A candidate that cannot report its schema fails after the old API stopped:
# the old API restarts and every silver file stays where it was.
previous_pid=$(jq -r .pid "$TEST_API_POINTER")
if bash "$repository/tools/restart-dev-api.sh" "$scratch/broken-release" fixture.toml \
    "$scratch/environment" "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" \
    --retire-incompatible-silver "$silver" 2>/dev/null; then
    echo 'Expected a candidate without a schema report to roll back.' >&2
    exit 1
fi
[[ -e "$silver/group/old.sqlite3" && -e "$silver/group/current.sqlite3" ]]
restarted=$(jq -r .pid "$TEST_API_POINTER")
[[ "$restarted" != "$previous_pid" && -e "/proc/$restarted" ]]
report=$(bash "$repository/tools/restart-dev-api.sh" "$scratch/release" fixture.toml \
    "$scratch/environment" "$TEST_API_POINTER" http://fixture TEST_API_TOKEN "$scratch/logs" \
    --retire-incompatible-silver "$silver")
[[ "$(jq -c .republish <<<"$report")" == '["old"]' ]]
[[ "$(jq -r .publications_preserved <<<"$report")" == false ]]
[[ ! -e "$silver/group/old.sqlite3" && -e "$silver/group/current.sqlite3" ]]
[[ -e "$silver/group/migratable.sqlite3" ]]
[[ -e "$(jq -r .retired_silver "$TEST_API_POINTER")/group/old.sqlite3" ]]
recorded_pid=$(jq -r .pid "$TEST_API_POINTER")
cp -f "$TEST_API_POINTER" "$scratch/valid-pointer.json"
jq '.pid=999999999' "$scratch/valid-pointer.json" > "$TEST_API_POINTER"
if bash "$repository/tools/check-dev-api.sh" "$scratch/environment" "$TEST_API_POINTER" \
    http://fixture TEST_API_TOKEN "$scratch/logs" > /dev/null 2> "$scratch/stale-error"; then
    cp -f "$scratch/valid-pointer.json" "$TEST_API_POINTER"
    echo 'Stale API process record was accepted.' >&2; exit 1
fi
cp -f "$scratch/valid-pointer.json" "$TEST_API_POINTER"
grep -Fq 'reconcile the service manager and release record' "$scratch/stale-error"
kill -0 "$recorded_pid"
