#!/usr/bin/env bash
set -euo pipefail
[[ $# == 3 ]] || { echo 'usage: install-dev-remote.sh SSH_HOST ARTIFACT REMOTE_DESTINATION' >&2; exit 2; }
host=$1 artifact=$2 destination=$3
# Use a fresh connection: a reused control master can retain disabled keepalives.
transport=(-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=2)
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$destination" =~ ^/[A-Za-z0-9/._-]+$ ]]
digest=$(sha256sum "$artifact")
digest=${digest%% *}
if ssh "${transport[@]}" "$host" \
    "test -f $destination && test -x $destination && test \$(shasum -a 256 $destination | cut -d ' ' -f 1) = $digest"; then
    exit 0
fi
incoming="$destination.incoming-$digest"
ssh "${transport[@]}" "$host" "mkdir -p $(dirname "$destination")"
scp "${transport[@]}" "$artifact" "$host:$incoming"
ssh "${transport[@]}" "$host" \
    "test \$(shasum -a 256 $incoming | cut -d ' ' -f 1) = $digest && chmod 755 $incoming && { test ! -f $destination || cp $destination $destination.previous; } && mv -f $incoming $destination"
