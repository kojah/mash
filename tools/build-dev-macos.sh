#!/usr/bin/env bash
set -euo pipefail
[[ $# == 3 ]] || { echo 'usage: build-dev-macos.sh SSH_HOST REMOTE_ROOT OUTPUT' >&2; exit 2; }
host=$1 root=$2 output=$3
# Use a fresh connection: a reused control master can retain disabled keepalives.
transport=(-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=2)
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$root" =~ ^/[A-Za-z0-9/._-]+$ ]]
revision=$(git rev-parse HEAD)
[[ "$revision" =~ ^[0-9a-f]{40}$ ]]
mkdir -p "$(dirname "$output")"
archive="$output.source.tar"
git archive --format=tar HEAD > "$archive"
ssh "${transport[@]}" "$host" "mkdir -p $root/source-$revision $root/build-source $root/build-tmp"
scp "${transport[@]}" "$archive" "$host:$root/source-$revision.tar"
ssh "${transport[@]}" "$host" \
    "tar -xf $root/source-$revision.tar -C $root/source-$revision && rsync -rlpc --delete $root/source-$revision/ $root/build-source/ && cd $root/build-source && export PATH=/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/bin:/bin && { ! command -v pnpm >/dev/null || (cd dashboard && pnpm install --frozen-lockfile && pnpm run build); } && TMPDIR=$root/build-tmp CARGO_TARGET_DIR=$root/target CARGO_INCREMENTAL=0 MASH_BUILD_REVISION=$revision cargo build --release --locked --bin mash"
# A stable source path and checksum synchronization preserve unchanged source
# mtimes, so Cargo can reuse its fingerprints across commits. Always run Cargo;
# this also checks toolchain, flags, dependency and changed-source inputs.
remote_digest=$(ssh "${transport[@]}" "$host" "shasum -a 256 $root/target/release/mash")
remote_digest=${remote_digest%% *}
[[ "$remote_digest" =~ ^[0-9a-f]{64}$ ]]
local_digest=''
if [[ -f "$output" ]]; then
    local_digest=$(sha256sum "$output")
    local_digest=${local_digest%% *}
fi
candidate=$output
if [[ "$local_digest" != "$remote_digest" ]]; then
    scp "${transport[@]}" "$host:$root/target/release/mash" "$output.incoming"
    incoming_digest=$(sha256sum "$output.incoming")
    [[ "${incoming_digest%% *}" == "$remote_digest" ]]
    chmod 755 "$output.incoming"
    candidate="$output.incoming"
fi
# Validate the 64-bit little-endian Mach-O magic and ARM64 CPU type without
# requiring the optional `file` utility on the deployment coordinator.
header=$(od -An -tx1 -N8 "$candidate" | tr -d ' \n')
[[ "$header" == cffaedfe0c000001 ]]
if [[ "$candidate" != "$output" ]]; then mv "$candidate" "$output"; fi
chmod 755 "$output"
printf '%s\n' "$revision" > "$output.revision"
sha256sum "$output" > "$output.sha256"
