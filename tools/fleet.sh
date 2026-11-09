#!/usr/bin/env bash
# Invoke the existing Ansible policy directly; retain private reports for review.
set -euo pipefail
[[ $# -ge 2 ]] || { echo 'usage: fleet.sh OPERATION TARGET...|--all' >&2; exit 2; }
operation=$1
shift
case "$operation" in
    preflight|deploy|check) playbook=$operation ;;
    sync|local-views) playbook=refresh ;;
    replay-views) playbook=replay-views ;;
    watch-start|watch-stop|watch-status|watch-logs) playbook=watch ;;
    *) echo 'Unknown fleet operation.' >&2; exit 2 ;;
esac
for target in "$@"; do
    [[ $# == 1 && "$target" == --all ]] && continue
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
export ANSIBLE_CONFIG="$repository/deployment/ansible.cfg"
source "$repository/tools/controller-environment.sh"
if [[ $# == 1 && "$1" == --all ]]; then
    uv run --project "$repository/deployment" --locked --no-sync \
        ansible-inventory --inventory "$inventory" --list > "$job/inventory.json"
    case "$operation" in
        sync|local-views|watch-*) selected_groups='["mash_history"]' ;;
        replay-views) selected_groups='["mash_api"]' ;;
        *) selected_groups='["mash_api","mash_history"]' ;;
    esac
    targets=()
    while IFS= read -r target; do targets+=("$target"); done < <(jq -r --argjson groups "$selected_groups" '
        . as $inventory |
        def hosts: . as $name | ($inventory[$name].hosts // [])[],
            (($inventory[$name].children // [])[] | hosts);
        [$groups[] | hosts] | unique[]' "$job/inventory.json")
    [[ ${#targets[@]} -gt 0 ]] || { echo 'No eligible inventory hosts.' >&2; exit 2; }
    set -- "${targets[@]}"
    for target in "$@"; do
        [[ "$target" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { echo 'Invalid inventory host name.' >&2; exit 2; }
    done
fi
jq -n --arg operation "$operation" --arg repository "$repository" --arg job "$job" \
    --arg fleet "${MASH_FLEET_BINARY:-}" \
    --args '{mash_operation:$operation,mash_repository:$repository,mash_job_directory:$job,mash_selected_nodes:$ARGS.positional} + (if $fleet == "" then {} else {mash_fleet:$fleet} end)' \
    "$@" > "$job/env/extravars"
result=0
bash "$repository/tools/run-ansible.sh" "$logs/fleet.lock" \
    timeout 7200 uv run --project "$repository/deployment" --locked --no-sync \
    ansible-runner run "$job" --project-dir "$repository" --inventory "$inventory" \
    --playbook "deployment/playbooks/$playbook.yml" --ident job --json \
    > "$job/runner.log" 2>&1 || result=$?
for target in "$@"; do
    report="$job/$target.json"
    if [[ -f "$report" ]]; then
        jq --arg target "$target" '{target:$target,state,error} + (if has("process") then {process,readiness,logs} else {} end)' "$report"
        jq -e '.state == "succeeded" or .state == "skipped"' "$report" > /dev/null || result=1
    else
        jq -n --arg target "$target" '{target:$target,state:"unverified"}'
        result=1
    fi
done
printf 'Private reports: %s\n' "$job" >&2
exit "$result"
