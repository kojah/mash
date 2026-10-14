#!/usr/bin/env bash
# One fleet mutation/check at a time across controller processes.
set -euo pipefail
[[ $# -ge 2 ]] || { echo 'usage: run-ansible.sh LOCK RUNNER ARGUMENTS...' >&2; exit 2; }
lock=$1
shift
mkdir -p "$(dirname "$lock")"
exec {fleet_lock}>"$lock"
flock -n "$fleet_lock" || { echo 'Another fleet job is active; poll it before retrying.' >&2; exit 75; }
exec "$@"
