#!/usr/bin/env bash
# Explicit, pinned tooling; bounded audits in an isolated source copy.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$root"
version=27.1.0
sha=dfe6dc37d0342c891d2829b5a695aa57c2d0edecef7e7d0399a30cc6e206411e
if [[ -d /home/james/scratch/mash-artifacts ]]; then
    default_artifacts=/home/james/scratch/mash-artifacts/mutations
else
    default_artifacts=${XDG_CACHE_HOME:-$HOME/.cache}/mash/mutations
fi
artifacts=${MASH_MUTATION_ARTIFACTS:-$default_artifacts}
mkdir -p "$artifacts"
artifacts=$(cd -- "$artifacts" && pwd -P)
case "$artifacts/" in "$root/"*) echo 'Mutation artifacts must be outside the checkout' >&2; exit 2;; esac
tool="$artifacts/tools/cargo-mutants"
if [[ ${1:-} == --install-tools ]]; then
    [[ $# == 1 && $(uname -s) == Linux && $(uname -m) == x86_64 ]] || {
        echo 'Pinned binary installation requires Linux x86_64' >&2; exit 2;
    }
    mkdir -p "$artifacts/tools"
    archive="$artifacts/tools/cargo-mutants.tar.gz"
    curl --fail --location --retry 3 --output "$archive" \
        "https://github.com/sourcefrog/cargo-mutants/releases/download/v$version/cargo-mutants-x86_64-unknown-linux-gnu.tar.gz"
    printf '%s  %s\n' "$sha" "$archive" | sha256sum --check --status
    tar -xzf "$archive" -C "$artifacts/tools" cargo-mutants
    "$tool" mutants --version
    exit
fi
[[ -x $tool ]] || { echo 'Run make mutation-tools first' >&2; exit 2; }
[[ $("$tool" mutants --version) == "cargo-mutants $version" ]] || {
    echo 'Unexpected cargo-mutants version' >&2; exit 2;
}
# No arbitrary arguments: keep baseline, source isolation and bounds mandatory.
[[ $# == 0 || ( $# == 1 && $1 == --list ) ]] || { echo 'Usage: mutation-check.sh [--install-tools|--list]' >&2; exit 2; }
if [[ ${1:-} == --list ]]; then
    exec "$tool" mutants --list
fi
mkdir -p "$artifacts/runs" "$artifacts/tmp"
run=$(mktemp -d "$artifacts/runs/audit.XXXXXX")
echo "Mutation evidence: $run"
export TMPDIR="$artifacts/tmp" CARGO_TARGET_DIR="$artifacts/target"
export CARGO_INCREMENTAL=1 RUSTC_WRAPPER=
# Serialize use of the persistent target even across separate agent sessions.
exec 9>"$artifacts/audit.lock"
flock 9
exec "$tool" mutants --jobs 1 --jobserver-tasks 4 --timeout 60 \
    --build-timeout 600 --output "$run"
