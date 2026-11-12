#!/usr/bin/env bash
# Carry a node's local index across a silver schema change. A schema the new
# release upgrades in place is left alone: indexing with that release performs
# the upgrade, and the deployment runs one pass for it. Any other incompatible
# schema is exported, and a staged import validated, before retiring any
# active files. Keep publication identity when the export actually carries it.
# Views are rebuilt afterwards by
# an explicit `mash index --rebuild-local-views`. A no-op when the new release
# can already open the index.
set -euo pipefail
umask 077
[[ $# == 4 ]] || {
    echo 'usage: upgrade-local-index.sh OLD_BINARY NEW_BINARY DATABASE BACKUP_ROOT' >&2
    exit 2
}
old=$1 new=$2 database=$3 backups=$4
marker="$database.upgrade-in-progress"
if [[ -e $marker ]]; then
    echo "unfinished local-index upgrade; inspect $marker/backup and recover retained files before retrying" >&2
    exit 1
fi
[[ -f $database ]] || exit 0
probe=$(mktemp)
export_config=$(mktemp)
trap 'rm -f "$probe" "$export_config"' EXIT
if "$new" --db "$database" status >/dev/null 2>"$probe"; then
    exit 0
fi
if ! grep -q '"kind":"schema-incompatible"' "$probe"; then
    echo 'new release cannot read the local index for a reason other than its schema' >&2
    cat "$probe" >&2
    exit 1
fi
# The release says so itself when it can upgrade this schema in place. Nothing
# is exported or moved for that; read-only commands never migrate.
if grep -q 'requires an in-place upgrade' "$probe"; then
    echo 'local index needs an in-place upgrade; indexing with the new release performs it'
    exit 0
fi
command -v jq >/dev/null
command -v lsof >/dev/null
quiescent() {
    local owners status=0
    owners=$(lsof -t -- "$database" "$database-wal" "$database-shm" 2>/dev/null) || status=$?
    if [[ -n $owners || $status -gt 1 ]]; then
        echo 'stop Mash index/watch/MCP processes before upgrading the local index' >&2
        return 1
    fi
}
# This check is not a fence against processes started later. Keep the node
# quiescent throughout export/cutover; old binaries do not honor our marker.
quiescent
mkdir -p "$backups"
backup=$(mktemp -d "$backups/local-index-$(date -u +%Y%m%dT%H%M%SZ).XXXXXXXX")
device() {
    case "$(uname -s)" in
        Darwin) stat -f %d "$1" ;;
        Linux) stat -c %d "$1" ;;
        *) echo 'unsupported platform for upgrade filesystem verification' >&2; return 1 ;;
    esac
}
active_device=$(device "$database")
backup_device=$(device "$backup")
if [[ $active_device != "$backup_device" ]]; then
    echo 'upgrade backup and active database must be on the same filesystem' >&2
    exit 1
fi
mkdir "$marker"
printf '%s\n' "$backup" > "$marker/backup"
retiring=false
cleanup() {
    rm -f "$probe" "$export_config"
    if ! $retiring; then
        rm -f "$marker/backup"
        rmdir "$marker"
    fi
}
trap cleanup EXIT
# Export reads canonical silver only. Give the old binary a minimal TOML
# instead of the live config, whose keys may belong to the new release.
printf 'version = 1\nproviders = {}\n' > "$export_config"
"$old" --config "$export_config" --db "$database" export "$backup/export" >"$backup/export.json"
if [[ ! -f $backup/export/silver.jsonl ]]; then
    echo 'export is not portable JSON Lines; use a compatible intermediate exporter for legacy format-1 indexes; active files were not changed' >&2
    exit 1
fi
identity=$(jq -nr 'any(inputs; .record == "publication")' "$backup/export/silver.jsonl")
if [[ $identity == true ]]; then set -- --keep-identity; else set --; fi
mkdir "$backup/staged" "$backup/retired"
staged="$backup/staged/$(basename "$database")"
"$new" --config "$export_config" --db "$staged" import "$backup/export" --into "$staged" "$@" \
    >"$backup/import.json"
[[ -f $staged && -d $staged.attachments ]]
quiescent
case "$database" in
    *.sqlite3) views="${database%.sqlite3}.views.sqlite3" ;;
    *) views="$database.views" ;;
esac
# Retire the pre-rename derived files as well; they are never imported.
retired_views="${database%.sqlite3}.gold.sqlite3"
retiring=true
for path in "$database" "$database-wal" "$database-shm" "$database-journal" \
    "$views" "$views-wal" "$views-shm" \
    "$retired_views" "$retired_views-wal" "$retired_views-shm" "$database.attachments"; do
    if [[ -e $path ]]; then
        mv "$path" "$backup/retired/"
    fi
done
[[ ! -e $database && ! -e $database.attachments ]]
mv "$staged.attachments" "$database.attachments"
# No-clobber installation on the same filesystem. Keep the staged file if the
# destination unexpectedly exists. Original files remain in retired/ throughout.
ln "$staged" "$database"
rm "$staged"
# One rename clears the active marker without a crash window that loses its
# recovery pointer. Keep that pointer with the completed backup as well.
mv "$marker" "$backup/completed-marker"
echo "local index upgraded; previous files retained in $backup"
