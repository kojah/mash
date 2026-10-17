#!/usr/bin/env bash
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:?}/dev-build-test.XXXXXXXX")
cleanup() {
    if [[ -f "$scratch/compiler-daemon.pid" ]]; then kill -TERM "$(cat "$scratch/compiler-daemon.pid")" 2>/dev/null || true; fi
    rm -rf "$scratch"
}
trap cleanup EXIT
mkdir -p "$scratch/repository/tools" "$scratch/repository/app" "$scratch/bin" "$scratch/target"
cp "$repository/tools/build-dev-linux.sh" "$scratch/repository/tools/"
cp "$repository/Makefile" "$scratch/repository/"
for helper in generate-history-skill test-dev-api test-dev-build test-run-ansible test-controller-environment test-ansible; do
    printf '#!/usr/bin/env bash\nexit 0\n' > "$scratch/repository/tools/$helper.sh"
done
printf 'printf "controller\\n" >> "$TEST_BUILD_CALLS"\n' > "$scratch/repository/tools/controller-environment.sh"
printf source > "$scratch/repository/app/source.rs"
git -C "$scratch/repository" init -q
git -C "$scratch/repository" add app/source.rs
fixture_commit() {
    git -C "$scratch/repository" -c core.hooksPath=/dev/null -c user.name=Fixture \
        -c user.email=fixture@example.invalid commit --allow-empty -qm fixture
}
fixture_commit
cat > "$scratch/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
if [[ $1 == --version ]]; then echo cargo-fixture; exit 0; fi
printf '%s\n' "$*" >> "$TEST_BUILD_CALLS"
if [[ $1 == test-clif && -e $TEST_BUILD_FAIL ]]; then exit 1; fi
if [[ $1 == clippy && -e $TEST_BUILD_CLIPPY_FAIL ]]; then exit 1; fi
if [[ $1 == test-clif && -e $TEST_BUILD_MUTATE ]]; then echo changed >> app/source.rs; fi
if [[ $1 == build && ! -e $TEST_BUILD_CHILD ]]; then
    sleep 60 >/dev/null 2>&1 &
    echo "$!" > "$TEST_BUILD_CHILD"
fi
CARGO
printf '#!/usr/bin/env bash\necho "${TEST_BUILD_COMPILER_VERSION:-rustc-fixture}"\n' > "$scratch/bin/rustc"
chmod 755 "$scratch/bin/cargo" "$scratch/bin/rustc"
export PATH="$scratch/bin:$PATH" CARGO_TARGET_DIR="$scratch/target"
export TEST_BUILD_CALLS="$scratch/calls" TEST_BUILD_FAIL="$scratch/fail" TEST_BUILD_MUTATE="$scratch/mutate"
export TEST_BUILD_CHILD="$scratch/compiler-daemon.pid"
export TEST_BUILD_CLIPPY_FAIL="$scratch/clippy-fail"
run_build() { timeout 10 bash "$scratch/repository/tools/build-dev-linux.sh" "$@"; }
timeout 10 make --no-print-directory -C "$scratch/repository" iterate DEV_RUN= \
    DEV_TMPDIR="$TMPDIR" DEV_TARGET_DIR="$scratch/target" FOCUS='test(contract)' >/dev/null
[[ $(sed -n '1p' "$TEST_BUILD_CALLS") == 'fmt --check' ]]
[[ $(sed -n '2p' "$TEST_BUILD_CALLS") == clippy* ]]
[[ $(sed -n '3p' "$TEST_BUILD_CALLS") == 'test-clif --locked -E test(contract) --no-tests fail --test-threads 4' ]]
[[ $(wc -l < "$TEST_BUILD_CALLS") == 3 ]]
[[ -z $(find "$scratch/target" -name '*.ok' -print -quit) ]]
: > "$TEST_BUILD_CALLS"
touch "$TEST_BUILD_CLIPPY_FAIL"
if run_build >/dev/null 2>&1; then echo 'Failed Clippy was accepted.' >&2; exit 1; fi
[[ $(wc -l < "$TEST_BUILD_CALLS") == 2 ]]
rm -f "$TEST_BUILD_CLIPPY_FAIL"
: > "$TEST_BUILD_CALLS"
run_build --validate-only >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 1 ]]
! grep -q '^build --release' "$TEST_BUILD_CALLS"
run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 1 ]]
run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 1 ]]
[[ $(rg -c '^build --release' "$TEST_BUILD_CALLS") == 2 ]]
fixture_commit
run_build >/dev/null
MASH_BUILD_REVISION=1111111111111111111111111111111111111111 run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 1 ]]
[[ $(rg -c '^build --release' "$TEST_BUILD_CALLS") == 4 ]]
run_build --validate-only >/dev/null
[[ $(rg -c '^build --release' "$TEST_BUILD_CALLS") == 4 ]]
printf changed >> "$scratch/repository/app/source.rs"
run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 2 ]]
touch "$TEST_BUILD_FAIL"
printf changed >> "$scratch/repository/app/source.rs"
if run_build >/dev/null 2>&1; then echo 'Failed gates were accepted.' >&2; exit 1; fi
rm "$TEST_BUILD_FAIL"
run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 4 ]]
touch "$TEST_BUILD_MUTATE"
printf changed >> "$scratch/repository/app/source.rs"
if run_build >/dev/null 2>&1; then echo 'Changing source was certified.' >&2; exit 1; fi
rm "$TEST_BUILD_MUTATE"
run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 6 ]]
TEST_BUILD_COMPILER_VERSION=rustc-new run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 7 ]]
RUSTFLAGS=test-flags run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 8 ]]
rm "$scratch/repository/app/source.rs"
run_build >/dev/null
[[ $(rg -c '^test-clif' "$TEST_BUILD_CALLS") == 9 ]]
echo 'Development build gate reuse and invalidation checks passed.'
