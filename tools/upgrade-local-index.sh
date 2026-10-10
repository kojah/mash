#!/usr/bin/env bash
# Carry a node's local index across a silver schema change. Mash has no
# in-place migrations: the installed release exports silver, the old files are
# retired to a private backup, and the new release imports the export into the
# same path, keeping the node's shared identity. View is rebuilt afterwards by
# an explicit `mash index --rebuild-local-views`. A no-op when the new release
# can already open the index.
set -euo pipefail
umask 077
[[ $# == 4 ]] || {
    echo 'usage: upgrade-local-index.sh OLD_BINARY NEW_BINARY DATABASE BACKUP_ROOT' >&2
    exit 2
}
old=$1 new=$2 database=$3 backups=$4
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
backup="$backups/local-index-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$backup"
# Export reads canonical silver only. Give the old binary a minimal TOML
# instead of the live config, whose keys may belong to the new release.
printf 'version = 1\nproviders = {}\n' > "$export_config"
"$old" --config "$export_config" --db "$database" export "$backup/export" >"$backup/export.json"
views="${database%.sqlite3}.views.sqlite3"
# Retire the pre-rename derived files as well; they are never imported.
retired_views="${database%.sqlite3}.gold.sqlite3"
for path in "$database" "$database-wal" "$database-shm" \
    "$views" "$views-wal" "$views-shm" \
    "$retired_views" "$retired_views-wal" "$retired_views-shm" "$database.attachments"; do
    if [[ -e $path ]]; then
        mv "$path" "$backup/"
    fi
done
"$new" --db "$database" import "$backup/export" --into "$database" --keep-identity \
    >"$backup/import.json"
echo "local index upgraded; previous files retained in $backup"
