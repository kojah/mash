#!/usr/bin/env bash
# Disposable process/lifecycle fixtures; never invokes the real systemctl.
set -euo pipefail
repository=$(cd "$(dirname "$0")/.." && pwd)
fixture_schema=$(sed -nE 's/^pub\(crate\) const SCHEMA_VERSION: i64 = ([0-9]+);$/\1/p' "$repository/app/storage/schema.rs")
[[ $fixture_schema =~ ^[1-9][0-9]*$ ]]
export TEST_SYSTEMD_SCHEMA=$fixture_schema
fixture_baseline=$(sed -nE 's/^const BASELINE: i64 = ([0-9]+);$/\1/p' "$repository/app/storage/schema.rs")
export TEST_SYSTEMD_BASELINE=$fixture_baseline
scratch=$(mktemp -d "${TMPDIR:?}/systemd-api-test.XXXXXXXX")
export TEST_SYSTEMD_ROOT=$scratch
mkdir -p "$scratch/bin" "$scratch/logs" "$scratch/silver/group" "$scratch/silver/sync"
trap 'if [[ -f $scratch/pid ]]; then kill "$(cat "$scratch/pid")" 2>/dev/null || true; fi' EXIT
cat > "$scratch/fixture.rs" <<'RS'
fn main() {
    // This stands in for a release from before --version-info, which rejects it.
    if std::env::args().any(|a| a == "--version-info") {
        std::process::exit(2);
    }
    if std::env::args().any(|a| a == "--version") {
        println!("{{\"silver_schema\":{},\"silver_schema_min\":{}}}", std::env::var("TEST_SYSTEMD_SCHEMA").unwrap(), std::env::var("TEST_SYSTEMD_BASELINE").unwrap());
        return;
    }
    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}
RS
rustc "$scratch/fixture.rs" -o "$scratch/artifact"
cp "$scratch/artifact" "$scratch/mash"
cat > "$scratch/bin/systemctl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
root=$TEST_SYSTEMD_ROOT
case $2 in
    is-active) [[ $(cat "$root/pid") != 0 ]] ;;
    show)
        if [[ $* == *FragmentPath* ]]; then echo "$root/unit.service";
        else cat "$root/pid"; fi
        ;;
    daemon-reload)
        if [[ ${TEST_FAIL_RELOAD:-0} == 1 ]]; then
            # Fail once so restoration can reload the original unit.
            if [[ ! -e $root/reload-failed ]]; then touch "$root/reload-failed"; exit 1; fi
        fi
        ;;
    stop)
        kill "$(cat "$root/pid")"
        printf '0\n' > "$root/pid"
        if [[ ${TEST_REMOVE_ARTIFACT:-0} == 1 ]]; then rm "$root/artifact"; fi
        ;;
    start)
        # The child must not retain the deployment lock.
        python3 - "$root" <<'PY'
import pathlib, subprocess, sys
root = pathlib.Path(sys.argv[1])
binary = (root / 'unit.service').read_text().strip().split('ExecStart=')[1]
p = subprocess.Popen([binary, '--server-config', 'fixture.toml'], stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     close_fds=True, start_new_session=True)
(root / 'pid').write_text(str(p.pid))
PY
        if [[ ${TEST_FAIL_AFTER_START:-0} == 1 ]]; then
            mkdir -p "$root/silver/group"
            echo accepted > "$root/silver/group/new-write"
            exit 1
        fi
        ;;
    *) exit 2 ;;
esac
SH
cat > "$scratch/bin/curl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
destination=''
authenticated=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --config) authenticated=true; cat >/dev/null; shift 2 ;;
        --output) destination=$2; shift 2 ;;
        *) shift ;;
    esac
done
if ! $authenticated; then printf 401; exit 0; fi
printf '{"nodes":[]}\n' > "$destination"
SH
chmod +x "$scratch/bin/"*
export PATH="$scratch/bin:$PATH"
echo 'TEST_TOKEN=fixture-token' > "$scratch/environment"
echo '{}' > "$scratch/pointer"
echo '# fixture' > "$scratch/config"
printf '[Service]\nExecStart=%s/mash\n' "$scratch" > "$scratch/unit.service"
systemctl --user start fixture.service
python3 - "$scratch/silver" <<'PY'
import pathlib, sqlite3, sys
root = pathlib.Path(sys.argv[1])
with sqlite3.connect(root / 'group/node.sqlite3') as db:
    db.execute('PRAGMA user_version=54')
with sqlite3.connect(root / 'sync/catalog.sqlite3') as db:
    db.execute('PRAGMA user_version=1')
    db.execute('CREATE TABLE receipt (value TEXT)')
    db.execute("INSERT INTO receipt VALUES ('retained')")
(root / 'sync/spool').write_text('retained transfer piece')
PY
run() {
    bash "$repository/tools/restart-systemd-api.sh" "$1" fixture.service \
        "$scratch/artifact" "$scratch/mash" "$scratch/config" "$scratch/environment" \
        "$scratch/pointer" http://fixture TEST_TOKEN "$scratch/logs" "$scratch/silver" "${@:2}"
}
if run preflight; then echo 'Incompatible state passed without opt-in' >&2; exit 1; fi
run preflight --retire-incompatible-silver
TEST_SYSTEMD_BASELINE=$fixture_schema
if run install --retire-incompatible-silver 2> "$scratch/range-error"; then
    echo 'Candidate with a different migration range passed source verification' >&2; exit 1
fi
grep -Fq 'Candidate schema range does not match preflight source' "$scratch/range-error"
TEST_SYSTEMD_BASELINE=$fixture_baseline
# Migration-capable schemas preserve the existing shared root and receipts.
for version in "$fixture_baseline" "$fixture_schema"; do
    python3 - "$scratch/silver/group/node.sqlite3" "$version" <<'PY'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    db.execute(f'PRAGMA user_version={int(sys.argv[2])}')
PY
    sha256sum "$scratch/silver/group/node.sqlite3" > "$scratch/migration-before.sha256"
    run preflight > "$scratch/migration-preflight"
    grep -Fq 'shared-state retirement required: false' "$scratch/migration-preflight"
    sha256sum --check "$scratch/migration-before.sha256" >/dev/null
done
python3 - "$scratch/silver/group/node.sqlite3" <<'PY'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    db.execute('PRAGMA user_version=54')
PY
[[ -f $scratch/silver/sync/catalog.sqlite3 ]]
live_pid=$(cat "$scratch/pid")
printf '%s\n' "$$" > "$scratch/pid"
if run check; then echo 'Accepted a different service executable' >&2; exit 1; fi
printf '%s\n' "$live_pid" > "$scratch/pid"
# Fail before activation after moving the root; restore both history and receipts.
export TEST_REMOVE_ARTIFACT=1
if run install --retire-incompatible-silver; then exit 1; fi
unset TEST_REMOVE_ARTIFACT
[[ ! -e $scratch/pointer.systemd-pending ]]
[[ -f $scratch/silver/group/node.sqlite3 && -f $scratch/silver/sync/catalog.sqlite3 ]]
[[ $(cat "$scratch/silver/sync/spool") == 'retained transfer piece' ]]
cp "$scratch/mash" "$scratch/artifact"
# A start failure can occur after the candidate accepted writes. Never roll back.
export TEST_FAIL_AFTER_START=1
printf 'TEST_TOKEN=next-fixture-token\n' > "$scratch/next-environment"
if run install --retire-incompatible-silver --next-environment "$scratch/next-environment"; then exit 1; fi
unset TEST_FAIL_AFTER_START
cmp "$scratch/environment" "$scratch/next-environment"
[[ $(cat "$scratch/silver/group/new-write") == accepted ]]
[[ ! -e $scratch/silver/sync/catalog.sqlite3 ]]
backup=$(cat "$scratch/pointer.systemd-pending")
[[ $(cat "$backup/phase") == activation-started ]]
[[ -f $backup/shared-state/group/node.sqlite3 && -f $backup/shared-state/sync/catalog.sqlite3 ]]
if run preflight --retire-incompatible-silver; then echo 'Ignored unfinished rollout' >&2; exit 1; fi
# Simulate operator-verified forward recovery, then exercise ordinary replacement.
rm "$scratch/pointer.systemd-pending"
run install
[[ $(jq -r .deployed "$scratch/pointer") == true ]]
[[ $(jq -r .republish_required "$scratch/pointer") == false ]]
[[ $(cat "$scratch/silver/group/new-write") == accepted ]]
live_pid=$(cat "$scratch/pid")
[[ $(run install | jq -r .unchanged) == true ]]
[[ $(cat "$scratch/pid") == "$live_pid" ]]
cp "$backup/shared-state/group/node.sqlite3" "$scratch/silver/group/node.sqlite3"
mkdir "$scratch/silver/sync"
cp "$backup/shared-state/sync/catalog.sqlite3" "$scratch/silver/sync/catalog.sqlite3"
run install --retire-incompatible-silver
[[ $(jq -r .republish_required "$scratch/pointer") == true ]]
retained=$(jq -r .backup "$scratch/pointer")
[[ -f $retained/shared-state/group/node.sqlite3 && -f $retained/shared-state/sync/catalog.sqlite3 ]]
[[ ! -e $scratch/silver/group/node.sqlite3 && ! -e $scratch/silver/sync/catalog.sqlite3 ]]
# Splitting the shared server from the CLI changes the unit only while stopped.
printf '[Service]\nExecStart=%s/mash-server\n' "$scratch" > "$scratch/next-unit"
split_run() {
    bash "$repository/tools/restart-systemd-api.sh" "$1" fixture.service \
        "$scratch/artifact" "$scratch/mash-server" "$scratch/config" "$scratch/environment" \
        "$scratch/pointer" http://fixture TEST_TOKEN "$scratch/logs" "$scratch/silver" \
        --previous-binary "$scratch/mash" --next-unit "$scratch/next-unit"
}
cp "$scratch/unit.service" "$scratch/unit-before"
sha256sum "$scratch/mash" > "$scratch/cli-before.sha256"
split_run preflight
export TEST_FAIL_RELOAD=1
if split_run install; then echo 'Ignored unit reload failure' >&2; exit 1; fi
unset TEST_FAIL_RELOAD
cmp "$scratch/unit.service" "$scratch/unit-before"
[[ $(readlink "/proc/$(cat "$scratch/pid")/exe") == "$scratch/mash" ]]
[[ ! -e $scratch/pointer.systemd-pending ]]
split_run install
cmp "$scratch/unit.service" "$scratch/next-unit"
sha256sum --check "$scratch/cli-before.sha256" >/dev/null
[[ $(readlink "/proc/$(cat "$scratch/pid")/exe") == "$scratch/mash-server" ]]
live_pid=$(cat "$scratch/pid")
[[ $(split_run install | jq -r .unchanged) == true ]]
[[ $(cat "$scratch/pid") == "$live_pid" ]]
echo 'Systemd deployment fixtures passed.'
