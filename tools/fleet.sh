#!/usr/bin/env bash
# Invoke the existing Ansible policy directly; retain private reports for review.
set -euo pipefail
[[ $# -ge 2 ]] || { echo 'usage: fleet.sh preflight|deploy|check|sync|local-views TARGET...' >&2; exit 2; }
operation=$1
shift
case "$operation" in
    preflight|deploy|check) playbook=$operation ;;
    sync|local-views) playbook=refresh ;;
    *) echo 'Unknown fleet operation.' >&2; exit 2 ;;
esac
for target in "$@"; do
    [[ "$target" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { echo 'Invalid target name.' >&2; exit 2; }
done
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
inventory=${MASH_DEPLOY_INVENTORY:-$HOME/.config/mash/dev-inventory.yml}
logs=${MASH_DEPLOY_LOGS:-/home/james/scratch/mash-artifacts/dev-jobs}
[[ -f "$inventory" ]] || { echo 'Set MASH_DEPLOY_INVENTORY to a private inventory.' >&2; exit 2; }
umask 077
mkdir -p "$logs"
job=$(mktemp -d "$logs/$operation.XXXXXXXX")
mkdir "$job/env"
jq -n --arg operation "$operation" --arg repository "$repository" --arg job "$job" \
    --args '{mash_operation:$operation,mash_repository:$repository,mash_job_directory:$job,mash_selected_nodes:$ARGS.positional}' \
    "$@" > "$job/env/extravars"
export ANSIBLE_CONFIG="$repository/deployment/ansible.cfg"
source "$repository/tools/controller-environment.sh"
result=0
bash "$repository/tools/run-ansible.sh" "$logs/fleet.lock" \
    timeout 7200 uv run --project "$repository/deployment" --locked --no-sync \
    ansible-runner run "$job" --project-dir "$repository" --inventory "$inventory" \
    --playbook "deployment/playbooks/$playbook.yml" --ident job --json \
    > "$job/runner.log" 2>&1 || result=$?
for target in "$@"; do
    report="$job/$target.json"
    if [[ -f "$report" ]]; then
        jq --arg target "$target" '{target:$target,state,error}' "$report"
        jq -e '.state == "succeeded" or .state == "skipped"' "$report" > /dev/null || result=1
    else
        jq -n --arg target "$target" '{target:$target,state:"unverified"}'
        result=1
    fi
done
printf 'Private reports: %s\n' "$job" >&2
exit "$result"
