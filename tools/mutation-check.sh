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
config=.cargo/mutants.toml
diff_args=()
if [[ -n ${MASH_MUTATION_DIFF:-} ]]; then
    [[ -f $MASH_MUTATION_DIFF ]] || { echo 'MASH_MUTATION_DIFF must name an existing unified diff' >&2; exit 2; }
    diff_args=(--in-diff "$MASH_MUTATION_DIFF")
fi
case "${1:-}" in
    '') [[ $# == 0 ]] || exit 2 ;;
    --list) [[ $# == 1 ]] || exit 2; exec "$tool" mutants --config "$config" "${diff_args[@]}" --list ;;
    --runtime|--list-runtime)
        [[ $# == 1 ]] || exit 2
        config=.cargo/mutants-runtime.toml
        if [[ $1 == --list-runtime ]]; then exec "$tool" mutants --config "$config" "${diff_args[@]}" --list; fi
        ;;
    --vec1|--list-vec1)
        [[ $# == 1 ]] || exit 2
        config=.cargo/mutants-vec1.toml
        if [[ $1 == --list-vec1 ]]; then exec "$tool" mutants --config "$config" "${diff_args[@]}" --list; fi
        ;;
    --connectors|--list-connectors)
        [[ $# == 1 ]] || exit 2
        config=.cargo/mutants-connectors.toml
        if [[ $1 == --list-connectors ]]; then exec "$tool" mutants --config "$config" "${diff_args[@]}" --list; fi
        [[ -n ${MASH_TEST_QDRANT_URL:-} ]] || { echo 'Set MASH_TEST_QDRANT_URL to disposable Qdrant 1.19+' >&2; exit 2; }
        ;;
    --certification|--list-certification)
        [[ $# == 1 ]] || exit 2
        config=.cargo/mutants-certification.toml
        if [[ $1 == --list-certification ]]; then exec "$tool" mutants --config "$config" "${diff_args[@]}" --list; fi
        [[ -n ${MASH_TEST_QDRANT_URL:-} ]] || { echo 'Set MASH_TEST_QDRANT_URL to disposable Qdrant 1.19+' >&2; exit 2; }
        ;;
    *) echo 'Usage: mutation-check.sh [--install-tools|--list|--runtime|--list-runtime|--vec1|--list-vec1|--connectors|--list-connectors|--certification|--list-certification]' >&2; exit 2 ;;
esac
mkdir -p "$artifacts/runs" "$artifacts/tmp"
run=$(mktemp -d "$artifacts/runs/audit.XXXXXX")
echo "Mutation evidence: $run"
export TMPDIR="$artifacts/tmp" CARGO_TARGET_DIR="$artifacts/target"
export CARGO_INCREMENTAL=1 RUSTC_WRAPPER=
# Serialize use of the persistent target even across separate agent sessions.
exec 9>"$artifacts/audit.lock"
flock 9
exec "$tool" mutants --config "$config" "${diff_args[@]}" --jobs 1 --jobserver-tasks 4 --timeout 120 \
    --build-timeout 600 --output "$run"
