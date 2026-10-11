#!/usr/bin/env bash
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:?}/ansible-lock-test.XXXXXXXX")
cleanup() { [[ -z ${runner_pid:-} ]] || kill -TERM "$runner_pid" 2>/dev/null || true; rm -rf "$scratch"; }
trap cleanup EXIT
bash "$repository/tools/run-ansible.sh" "$scratch/fleet.lock" sh -c "touch '$scratch/started'; sleep 1" &
runner_pid=$!
for ((attempt=0; attempt<50; attempt++)); do [[ ! -e "$scratch/started" ]] || break; sleep 0.01; done
[[ -e "$scratch/started" ]]
if bash "$repository/tools/run-ansible.sh" "$scratch/fleet.lock" /usr/bin/true; then echo 'Concurrent fleet jobs were accepted.' >&2; exit 1; fi
wait "$runner_pid"
runner_pid=''
bash "$repository/tools/run-ansible.sh" "$scratch/fleet.lock" /usr/bin/true
echo 'Ansible fleet lock rejects overlapping sessions and releases at completion.'
