#!/usr/bin/env bash
set -euo pipefail
[[ $# == 3 ]] || { echo 'usage: build-dev-macos.sh SSH_HOST REMOTE_ROOT OUTPUT' >&2; exit 2; }
host=$1 root=$2 output=$3
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$root" =~ ^/[A-Za-z0-9/._-]+$ ]]
revision=$(git rev-parse HEAD)
[[ "$revision" =~ ^[0-9a-f]{40}$ ]]
mkdir -p "$(dirname "$output")"
archive="$output.source.tar"
git archive --format=tar HEAD > "$archive"
ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" "mkdir -p $root/source-$revision $root/build-tmp"
scp -o BatchMode=yes -o ConnectTimeout=10 "$archive" "$host:$root/source-$revision.tar"
ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" \
    "tar -xf $root/source-$revision.tar -C $root/source-$revision && cd $root/source-$revision && PATH=/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/bin:/bin TMPDIR=$root/build-tmp CARGO_TARGET_DIR=$root/target CARGO_INCREMENTAL=0 cargo build --release --locked --bin mash"
scp -o BatchMode=yes -o ConnectTimeout=10 "$host:$root/target/release/mash" "$output.incoming"
chmod 755 "$output.incoming"
file "$output.incoming" | grep -q arm64
mv "$output.incoming" "$output"
printf '%s\n' "$revision" > "$output.revision"
sha256sum "$output" > "$output.sha256"
