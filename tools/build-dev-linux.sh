#!/usr/bin/env bash
# Reuse successful validation only for identical inputs; always build/check the artifact.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
: "${CARGO_TARGET_DIR:?Set the persistent target directory}"
: "${TMPDIR:?Set the disk-backed test temporary directory}"
mkdir -p "$CARGO_TARGET_DIR"
exec {build_lock}>"$CARGO_TARGET_DIR/mash-development-build.lock"
flock "$build_lock"
run() {
    # Compiler cache servers and fixture subprocesses must not retain this lock.
    "$@" {build_lock}>&-
}
validation_key() {
    {
        git ls-files --cached --others --exclude-standard -z -- app crates queries plugins deployment tools .cargo .config .github Cargo.toml Cargo.lock Makefile rust-toolchain.toml build.rs |
            sort -zu | while IFS= read -r -d '' path; do
                if [[ -f "$path" ]]; then sha256sum -- "$path" || return
                else printf 'deleted %s\n' "$path"
                fi
            done || return
        rustc -vV || return
        cargo --version || return
        # Hash effective compiler/profile settings without retaining raw env values.
        env -0 | LC_ALL=C sort -z | LC_ALL=C grep -zE '^(CARGO_|RUST|MASH_TEST_|MASH_BUILD_REVISION=|CC=|CXX=|CFLAGS=|CXXFLAGS=|AR=|SCCACHE_)' || return
        printf '%s\n' "target=$CARGO_TARGET_DIR" "flags=${RUSTFLAGS:-}" "wrapper=${RUSTC_WRAPPER:-}" "toolchain=${RUSTUP_TOOLCHAIN:-}" "build_target=${CARGO_BUILD_TARGET:-}" "incremental=${CARGO_INCREMENTAL:-}" "encoded_flags=${CARGO_ENCODED_RUSTFLAGS:-}" "revision_override=${MASH_BUILD_REVISION:-}"
    } | sha256sum | cut -d ' ' -f 1
}
key=$(validation_key)
receipt="$CARGO_TARGET_DIR/mash-development-gates-$key.ok"
if [[ -f "$receipt" && $(cat "$receipt") == "$key" ]]; then
    echo 'Reusing successful development gates for identical source and toolchain inputs.'
else
    run cargo fmt --check
    run bash tools/generate-history-skill.sh --check
    run bash tools/test-dev-api.sh
    run bash tools/test-dev-build.sh
    run bash tools/test-run-ansible.sh
    run bash tools/test-ansible.sh
    run cargo test-clif --locked --profile ci
    RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Znext-solver=no" run cargo clippy --all-targets -- -D warnings
    run cargo test --doc
    [[ $(validation_key) == "$key" ]] || { echo 'Source changed during validation; rerun the build gate.' >&2; exit 1; }
    printf '%s\n' "$key" > "$receipt.incoming.$$"
    mv "$receipt.incoming.$$" "$receipt"
fi
# Commit identity can change while source stays identical. Cargo's build script
# embeds the current revision; gate reuse never means reusing a differently stamped binary.
run cargo build --release --locked --bin mash --bin mash-dev-probe
[[ $(validation_key) == "$key" ]] || { echo 'Source changed during build; do not install this artifact.' >&2; exit 1; }
