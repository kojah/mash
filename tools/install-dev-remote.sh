#!/usr/bin/env bash
set -euo pipefail
[[ $# == 3 ]] || { echo 'usage: install-dev-remote.sh SSH_HOST ARTIFACT REMOTE_DESTINATION' >&2; exit 2; }
host=$1 artifact=$2 destination=$3
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$destination" =~ ^/[A-Za-z0-9/._-]+$ ]]
digest=$(sha256sum "$artifact")
digest=${digest%% *}
incoming="$destination.incoming-$digest"
ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" "mkdir -p $(dirname "$destination")"
scp -o BatchMode=yes -o ConnectTimeout=10 "$artifact" "$host:$incoming"
ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" \
    "test \$(shasum -a 256 $incoming | cut -d ' ' -f 1) = $digest && chmod 755 $incoming && { test ! -f $destination || cp $destination $destination.previous; } && mv -f $incoming $destination"
