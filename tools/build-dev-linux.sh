#!/usr/bin/env bash
# Reuse successful validation only for identical inputs; always build/check the artifact.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mode=release
focus=''
case "${1:-}" in
    --focused)
        [[ ($# == 2 || ($# == 3 && $3 == --all-targets)) && -n $2 ]] || { echo 'Set one nonempty nextest filter, optionally followed by --all-targets.' >&2; exit 2; }
        mode=focused; focus=$2
        # Unit tests need only the library test binaries. Building every target
        # also compiles the library a second time and links both executables,
        # the CLI contract test and the example. A filter that names a binary
        # or target kind selects integration targets, so it builds them all.
        scope=(--lib)
        if [[ $# == 3 || $focus == *binary* || $focus == *kind\(* ]]; then scope=(); fi ;;
    --validate-only) [[ $# == 1 ]] || exit 2; mode=validate ;;
    '') [[ $# == 0 ]] || exit 2 ;;
    *) echo 'usage: build-dev-linux.sh [--focused NEXTEST_FILTER [--all-targets] | --validate-only]' >&2; exit 2 ;;
esac
: "${CARGO_TARGET_DIR:?Set the persistent target directory}"
: "${TMPDIR:?Set the disk-backed test temporary directory}"
mkdir -p "$CARGO_TARGET_DIR"
exec {build_lock}>"$CARGO_TARGET_DIR/mash-development-build.lock"
flock "$build_lock"
run() {
    # Compiler cache servers and fixture subprocesses must not retain this lock.
    "$@" {build_lock}>&-
}
cheap_checks() {
    run cargo fmt --check
    RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Znext-solver=no" run cargo clippy --all-targets -- -D warnings
}
if [[ $mode == focused ]]; then
    run cargo fmt --check
    # One crate's tests need only that crate and the ones below it. Building
    # every crate's tests also compiles the application library twice, with
    # and without its tests, so the library alone is the default.
    # Integration targets belong to the executables' crate, so asking for
    # them builds every crate unless one is named.
    chosen=${MASH_ITERATE_PACKAGE:-}
    if [[ -z $chosen ]]; then
        if (( ${#scope[@]} )); then chosen=mash-app; else chosen=all; fi
    fi
    package=(-p "$chosen")
    [[ $chosen != all ]] || package=()
    if (( ${#scope[@]} )); then
        echo "Building library unit tests of $chosen only; set PACKAGE to another crate (or all), and TARGETS=all (or name a binary in FOCUS) for the CLI contract and other integration targets."
    fi
    # Clippy and the test build share no artifacts, so they run at the same
    # time and one loop reports both. Cargo locks a target directory for a
    # whole build, so Clippy gets its own. With a launcher each is its own
    # job: together in one they would exceed a single job's memory limit.
    launch=()
    [[ -z ${MASH_ITERATE_LAUNCHER:-} ]] || launch=("$MASH_ITERATE_LAUNCHER" env "TMPDIR=$TMPDIR")
    clippy_log=$(mktemp "$TMPDIR/mash-iterate-clippy.XXXXXXXX")
    CARGO_TARGET_DIR="$CARGO_TARGET_DIR/clippy" RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Znext-solver=no" \
        run "${launch[@]}" cargo clippy --all-targets -- -D warnings >"$clippy_log" 2>&1 &
    clippy_job=$!
    tests=0
    run "${launch[@]}" cargo test-clif --locked "${package[@]}" "${scope[@]}" -E "$focus" --no-tests fail --test-threads 4 || tests=$?
    clippy=0
    wait "$clippy_job" || clippy=$?
    echo "--- cargo clippy (exit $clippy) ---"
    cat "$clippy_log"
    rm -f -- "$clippy_log"
    (( clippy == 0 )) || echo 'Clippy failed.' >&2
    (( tests == 0 )) || echo 'Tests failed.' >&2
    (( clippy == 0 && tests == 0 )) || exit 1
    exit 0
fi
validation_key() {
    {
        git ls-files --cached --others --exclude-standard -z -- app cli crates queries plugins deployment tools .cargo .config .github Cargo.toml Cargo.lock Makefile rust-toolchain.toml build.rs |
            sort -zu | while IFS= read -r -d '' path; do
                if [[ -f "$path" ]]; then sha256sum -- "$path" || return
                else printf 'deleted %s\n' "$path"
                fi
            done || return
        rustc -vV || return
        cargo --version || return
        # Hash effective compiler/profile settings without retaining raw env values.
        env -0 | LC_ALL=C sort -z | LC_ALL=C grep -zE '^(CARGO_|RUST|MASH_TEST_|MASH_CONTRACT_BASE=|CC=|CXX=|CFLAGS=|CXXFLAGS=|AR=|SCCACHE_)' || return
        printf '%s\n' "target=$CARGO_TARGET_DIR" "flags=${RUSTFLAGS:-}" "wrapper=${RUSTC_WRAPPER:-}" "toolchain=${RUSTUP_TOOLCHAIN:-}" "build_target=${CARGO_BUILD_TARGET:-}" "incremental=${CARGO_INCREMENTAL:-}" "encoded_flags=${CARGO_ENCODED_RUSTFLAGS:-}"
    } | sha256sum | cut -d ' ' -f 1
}
key=$(validation_key)
receipt="$CARGO_TARGET_DIR/mash-development-gates-$key.ok"
if [[ -f "$receipt" && $(cat "$receipt") == "$key" ]]; then
    echo 'Reusing successful development gates for identical source and toolchain inputs.'
else
    cheap_checks
    source tools/controller-environment.sh
    run bash tools/test-dev-api.sh
    run bash tools/test-systemd-api.sh
    run bash tools/test-dev-build.sh
    run bash tools/test-install-shell.sh
    run bash tools/test-run-ansible.sh
    run bash tools/test-controller-environment.sh
    run bash tools/test-ansible.sh
    run bash tools/test-upgrade-local-index.sh
    run bash tools/generate-history-skill.sh --check
    run cargo test-clif --locked --profile ci --test-threads 4
    run cargo test --doc
    [[ $(validation_key) == "$key" ]] || { echo 'Source changed during validation; rerun the build gate.' >&2; exit 1; }
    printf '%s\n' "$key" > "$receipt.incoming.$$"
    mv "$receipt.incoming.$$" "$receipt"
fi
if [[ $mode == validate ]]; then
    echo 'Validation complete; no release binary built.'
    exit 0
fi
# Commit identity can change while source stays identical. Cargo's build script
# embeds the current revision; gate reuse never means reusing a differently stamped binary.
# The release embeds the dashboard front end (see app/build.rs), so build it first.
run bash -c 'cd dashboard && pnpm install --frozen-lockfile && pnpm run build'
run cargo build --release --locked --bin mash --bin mash-server --bin mash-fleet
[[ $(validation_key) == "$key" ]] || { echo 'Source changed during build; do not install this artifact.' >&2; exit 1; }
