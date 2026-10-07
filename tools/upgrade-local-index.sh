#!/usr/bin/env bash
# Carry a node's local index across a silver schema change. Mash has no
# in-place migrations: the installed release exports silver, the old files are
# retired to a private backup, and the new release imports the export into the
# same path, keeping the node's shared identity. Gold is rebuilt afterwards by
# an explicit `mash index --rebuild-local-gold`. A no-op when the new release
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
trap 'rm -f "$probe"' EXIT
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
"$old" --db "$database" export "$backup/export" >"$backup/export.json"
gold="${database%.sqlite3}.gold.sqlite3"
for path in "$database" "$database-wal" "$database-shm" \
    "$gold" "$gold-wal" "$gold-shm" "$database.attachments"; do
    if [[ -e $path ]]; then
        mv "$path" "$backup/"
    fi
done
"$new" --db "$database" import "$backup/export" --into "$database" --keep-identity \
    >"$backup/import.json"
echo "local index upgraded; previous files retained in $backup"
