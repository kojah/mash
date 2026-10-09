#!/usr/bin/env bash
# A surviving child must not retain its completed foreground job's admission.
set -euo pipefail
umask 077
test_launcher="${AGENT_TEST_LAUNCHER:-$HOME/.local/bin/agent-test}"
test_artifacts="${MASH_TEST_ARTIFACTS:-/home/james/scratch/mash-artifacts}"
mkdir -p "$test_artifacts"
test_directory="$(mktemp -d "$test_artifacts/agent-test-locks.XXXXXX")"
test_parent=
test_child=
cleanup() {
    touch "$test_directory/release"
    if [[ -n $test_child ]]; then kill "$test_child" 2>/dev/null || true; fi
    if [[ -n $test_parent ]]; then kill "$test_parent" 2>/dev/null || true; fi
}
trap cleanup EXIT

"$test_launcher" bash -c '
    set -euo pipefail
    for descriptor in /proc/self/fd/*; do
        target=$(readlink "$descriptor" || true)
        case "$target" in
            */agent-tests/lane-*.lock)
                echo "workload inherited admission lock" >&2; exit 42 ;;
        esac
    done
    sleep 300 >/dev/null 2>&1 &
    echo "$!" > "$1/child"
    touch "$1/started"
    while [[ ! -e $1/release ]]; do sleep 0.05; done
' admission-regression "$test_directory" > "$test_directory/output" 2>&1 &
test_parent=$!
for ((attempt = 0; attempt < 2400; attempt++)); do
    if [[ -e $test_directory/started ]]; then break; fi
    if ! kill -0 "$test_parent" 2>/dev/null; then
        cat "$test_directory/output" >&2
        echo "launcher exited before regression workload started" >&2
        exit 1
    fi
    sleep 0.05
done
[[ -e $test_directory/started ]]
read -r test_child < "$test_directory/child"

# Admission must still belong to the foreground launcher while work is active.
test_lock=
for descriptor in /proc/"$test_parent"/fd/*; do
    target="$(readlink "$descriptor" || true)"
    case "$target" in */agent-tests/lane-*.lock) test_lock=$target ;; esac
done
[[ -n $test_lock ]]
if flock -n "$test_lock" true; then
    echo "foreground workload lost admission before completion" >&2
    exit 1
fi
touch "$test_directory/release"
wait "$test_parent"
test_parent=

# The background child deliberately remains alive after its parent has exited.
kill -0 "$test_child"
for descriptor in /proc/"$test_child"/fd/*; do
    target="$(readlink "$descriptor" || true)"
    case "$target" in
        */agent-tests/lane-*.lock)
            echo "surviving child retained admission lock" >&2; exit 1 ;;
    esac
done
printf 'PASS: foreground admission retained; surviving child owns no slot lock.\n'
cat "$test_directory/output"
