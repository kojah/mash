#!/usr/bin/env bash
# Carry a remote node's local index across a silver schema change before its
# binary is replaced: run upgrade-local-index.sh there with the installed
# release as the exporter and the new artifact as the importer. A no-op when
# the new release can already open the index, or when the node has none.
set -euo pipefail
[[ $# == 3 || $# == 4 ]] || {
    echo 'usage: upgrade-remote-index.sh SSH_HOST ARTIFACT INSTALLED_BINARY [REMOTE_DATABASE]' >&2
    exit 2
}
host=$1 artifact=$2 installed=$3 database=${4:-}
# Use a fresh connection: a reused control master can retain disabled keepalives.
transport=(-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=2)
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$installed" =~ ^/[A-Za-z0-9/._-]+$ ]]
digest=$(sha256sum "$artifact")
digest=${digest%% *}
staging="/tmp/mash-upgrade-$digest"
ssh "${transport[@]}" "$host" "mkdir -p $staging && chmod 700 $staging"
scp "${transport[@]}" "$artifact" "$host:$staging/mash"
scp "${transport[@]}" "$(dirname "$0")/upgrade-local-index.sh" "$host:$staging/upgrade-local-index.sh"
# The default database matches Mash's own platform data directory, so a node
# that never passed --db is upgraded where its release reads it.
remote=$(cat <<'REMOTE'
set -euo pipefail
staging=$1 installed=$2 database=$3 digest=$4
trap 'rm -rf "$staging"' EXIT
test "$(shasum -a 256 "$staging/mash" | cut -d ' ' -f 1)" = "$digest"
chmod 755 "$staging/mash"
if [[ -z $database ]]; then
    case "$(uname -s)" in
        Darwin) database="$HOME/Library/Application Support/dev.jamesqo.mash/mash.sqlite3" ;;
        *) database="${XDG_DATA_HOME:-$HOME/.local/share}/mash/mash.sqlite3" ;;
    esac
fi
if [[ ! -x $installed ]]; then
    echo "no installed release at $installed; nothing to upgrade"
else
    bash "$staging/upgrade-local-index.sh" "$installed" "$staging/mash" "$database" \
        "$(dirname "$database")/local-index-upgrades"
fi
REMOTE
)
ssh "${transport[@]}" "$host" bash -s -- "$staging" "$installed" "$(printf %q "$database")" "$digest" <<<"$remote"
