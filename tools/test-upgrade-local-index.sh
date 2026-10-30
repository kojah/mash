#!/usr/bin/env bash
# Exercise durable file boundaries with disposable data and fake CLI processes.
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:?}/upgrade-local-test.XXXXXXXX")
trap 'rm -rf "$scratch"' EXIT
mkdir "$scratch/bin"
real_mv=$(command -v mv)
export TEST_UPGRADE_REAL_MV="$real_mv"
cat > "$scratch/bin/lsof" <<'SH'
#!/usr/bin/env bash
if [[ -e $TEST_UPGRADE_ROOT/busy ]]; then echo 123; exit 0; fi
exit 1
SH
cat > "$scratch/bin/mv" <<'SH'
#!/usr/bin/env bash
if [[ -e $TEST_UPGRADE_ROOT/interrupt && $1 == "$TEST_UPGRADE_ROOT/db.sqlite3-wal" ]]; then exit 1; fi
if [[ -e $TEST_UPGRADE_ROOT/finish-fail && $1 == "$TEST_UPGRADE_ROOT/db.sqlite3.upgrade-in-progress" ]]; then exit 1; fi
exec "$TEST_UPGRADE_REAL_MV" "$@"
SH
cat > "$scratch/bin/mash" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case " $* " in
    *' status '*) printf '{"error":{"kind":"schema-incompatible"}}\n' >&2; exit 1 ;;
    *' export '*)
        destination=${!#}
        mkdir "$destination"
        if [[ -e $TEST_UPGRADE_ROOT/legacy ]]; then exit 0; fi
        printf '{"record":"state"}\n' > "$destination/silver.jsonl"
        if [[ -e $TEST_UPGRADE_ROOT/identity ]]; then printf '{"record":"publication"}\n' >> "$destination/silver.jsonl"; fi
        ;;
    *' import '*)
        printf '%s\n' "$*" > "$TEST_UPGRADE_ROOT/import-args"
        if [[ -e $TEST_UPGRADE_ROOT/reject ]]; then exit 1; fi
        while [[ $1 != --into ]]; do shift; done
        printf new > "$2"
        mkdir "$2.attachments"
        printf new-blob > "$2.attachments/blob"
        ;;
    *) exit 2 ;;
esac
SH
chmod +x "$scratch/bin/"*
export PATH="$scratch/bin:$PATH"
fixture() {
    export TEST_UPGRADE_ROOT="$scratch/$1"
    mkdir -p "$TEST_UPGRADE_ROOT/db.sqlite3.attachments"
    printf original > "$TEST_UPGRADE_ROOT/db.sqlite3"
    printf wal > "$TEST_UPGRADE_ROOT/db.sqlite3-wal"
    printf old-blob > "$TEST_UPGRADE_ROOT/db.sqlite3.attachments/blob"
}
upgrade() {
    bash "$repository/tools/upgrade-local-index.sh" "$scratch/bin/mash" "$scratch/bin/mash" \
        "$TEST_UPGRADE_ROOT/db.sqlite3" "$TEST_UPGRADE_ROOT/backups" > "$TEST_UPGRADE_ROOT/result" 2> "$TEST_UPGRADE_ROOT/error"
}
unchanged() {
    [[ $(cat "$TEST_UPGRADE_ROOT/db.sqlite3") == original ]]
    [[ $(cat "$TEST_UPGRADE_ROOT/db.sqlite3-wal") == wal ]]
    [[ $(cat "$TEST_UPGRADE_ROOT/db.sqlite3.attachments/blob") == old-blob ]]
}
for failure in reject legacy busy; do
    fixture "$failure"
    touch "$TEST_UPGRADE_ROOT/$failure"
    if upgrade; then echo "Accepted $failure" >&2; exit 1; fi
    unchanged
    [[ ! -e $TEST_UPGRADE_ROOT/db.sqlite3.upgrade-in-progress ]]
done
for identity in absent present; do
    fixture "identity-$identity"
    if [[ $identity == present ]]; then touch "$TEST_UPGRADE_ROOT/identity"; fi
    upgrade
    [[ $(cat "$TEST_UPGRADE_ROOT/db.sqlite3") == new ]]
    [[ $(cat "$TEST_UPGRADE_ROOT/db.sqlite3.attachments/blob") == new-blob ]]
    [[ ! -e $TEST_UPGRADE_ROOT/db.sqlite3.upgrade-in-progress ]]
    if [[ $identity == present ]]; then
        grep -q -- --keep-identity "$TEST_UPGRADE_ROOT/import-args"
    else
        ! grep -q -- --keep-identity "$TEST_UPGRADE_ROOT/import-args"
    fi
    retired=("$TEST_UPGRADE_ROOT"/backups/*/retired)
    [[ $(cat "${retired[0]%/retired}/completed-marker/backup") == "${retired[0]%/retired}" ]]
    [[ $(cat "${retired[0]}/db.sqlite3") == original ]]
    [[ $(cat "${retired[0]}/db.sqlite3-wal") == wal ]]
    [[ $(cat "${retired[0]}/db.sqlite3.attachments/blob") == old-blob ]]
done
fixture interrupted
touch "$TEST_UPGRADE_ROOT/interrupt"
if upgrade; then echo 'Accepted interrupted cutover' >&2; exit 1; fi
marker="$TEST_UPGRADE_ROOT/db.sqlite3.upgrade-in-progress"
[[ -d $marker && ! -e $TEST_UPGRADE_ROOT/db.sqlite3 ]]
backup=$(cat "$marker/backup")
[[ $(cat "$backup/retired/db.sqlite3") == original ]]
[[ $(cat "$TEST_UPGRADE_ROOT/db.sqlite3-wal") == wal ]]
[[ $(cat "$backup/staged/db.sqlite3") == new ]]
rm "$TEST_UPGRADE_ROOT/interrupt"
if upgrade; then echo 'Ignored unfinished marker when active database was absent' >&2; exit 1; fi
grep -q 'unfinished local-index upgrade' "$TEST_UPGRADE_ROOT/error"
fixture finish-failure
touch "$TEST_UPGRADE_ROOT/finish-fail"
if upgrade; then echo 'Accepted failed marker retirement' >&2; exit 1; fi
marker="$TEST_UPGRADE_ROOT/db.sqlite3.upgrade-in-progress"
backup=$(cat "$marker/backup")
[[ $(cat "$TEST_UPGRADE_ROOT/db.sqlite3") == new ]]
[[ $(cat "$backup/retired/db.sqlite3") == original ]]
if upgrade; then echo 'Ignored retained marker after database promotion' >&2; exit 1; fi
echo 'Upgrade staging, identity, quiescence, retention and interrupted retry checks passed.'
