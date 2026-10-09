#!/bin/bash
# Run actual test commands here, rather than wrappers that create other cgroups.
set -euo pipefail
if [[ ${1:-} == -- ]]; then shift; fi
if (( $# == 0 )); then
    echo 'Usage: agent-test COMMAND [ARGUMENTS...]' >&2
    exit 2
fi
if [[ ${AGENT_TEST_ACTIVE:-} == 1 ]]; then
    exec "$@"
fi
test_state="$HOME/.local/state/agent-tests"
test_tmp="$HOME/.local/tmp/agent-tests"
mkdir -p "$test_state" "$test_tmp"
if [[ $(stat -f -c %T "$test_tmp") == tmpfs ]]; then
    echo 'Refusing to run: agent test TMPDIR is RAM-backed.' >&2
    exit 2
fi
if [[ $(systemctl --user show agent-tests.slice -p MemoryMax --value) != 34359738368 ]]; then
    echo 'Shared 32 GiB test budget is missing. Reload the systemd user configuration.' >&2
    exit 2
fi
test_lane_fd=
while [[ -z $test_lane_fd ]]; do
    for test_lane in 1 2 3 4; do
        exec {candidate_fd}>"$test_state/lane-$test_lane.lock"
        if flock -n "$candidate_fd"; then
            test_lane_fd=$candidate_fd
            break
        fi
        exec {candidate_fd}>&-
    done
    if [[ -z $test_lane_fd ]]; then
        if [[ ${test_wait_announced:-0} == 0 ]]; then
            echo 'All four agent test slots are busy; waiting.' >&2
            test_wait_announced=1
        fi
        sleep 1
    fi
done
test_unit="agent-test-${BASHPID}-$RANDOM"
# Only this launcher owns admission. Children may outlive the foreground job,
# so close its lock descriptor in systemd-run while the parent retains it.
systemd-run --user --scope --quiet --unit="$test_unit" \
    --slice=agent-tests.slice \
    -p MemoryAccounting=yes -p MemoryHigh=6G -p MemoryMax=8G -p MemorySwapMax=2G \
    /usr/bin/env AGENT_TEST_ACTIVE=1 TMPDIR="$test_tmp" \
    GOMEMLIMIT="${GOMEMLIMIT:-4GiB}" GOMAXPROCS="${GOMAXPROCS:-4}" GOGC="${GOGC:-100}" \
    /bin/bash -c '
        "$@"
        test_status=$?
        test_cgroup=$(awk -F: '\''$1 == "0" {print $3}'\'' /proc/self/cgroup)
        test_peak_path="/sys/fs/cgroup${test_cgroup}/memory.peak"
        if [[ -r $test_peak_path ]] && read -r test_peak < "$test_peak_path" && [[ $test_peak =~ ^[0-9]+$ ]]; then
            printf "agent-test: peak cgroup memory %s bytes; exit %s\n" "$test_peak" "$test_status" >&2
        else
            echo "agent-test: ERROR: peak memory unavailable; measurement is unknown." >&2
            [[ $test_status != 0 ]] || test_status=125
        fi
        exit "$test_status"
    ' agent-test-command "$@" {test_lane_fd}>&-
