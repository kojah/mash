#!/usr/bin/env bash
# Exercise fleet policy with real Ansible and disposable, non-history targets.
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:?}/ansible-fleet-test.XXXXXXXX")
trap 'rm -rf "$scratch"' EXIT
cat > "$scratch/probe" <<'PROBE'
#!/usr/bin/env bash
set -euo pipefail
output='' mode=''
while [[ $# -gt 0 ]]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        --mode) mode=$2; shift 2 ;;
        *) shift ;;
    esac
done
if [[ "$mode" == local-views && -e "$TEST_ANSIBLE_UPGRADE" ]]; then exit 1; fi
printf '%s\n' "$mode" >> "$TEST_ANSIBLE_ACTIONS"
printf '%s\n' '{"state":"succeeded","results":[]}' > "$output"
PROBE
chmod 755 "$scratch/probe"
export TEST_ANSIBLE_ACTIONS="$scratch/actions"
export TEST_ANSIBLE_UPGRADE="$scratch/upgrade"
cat > "$scratch/inventory.yml" <<INVENTORY
all:
  vars:
    ansible_connection: local
    ansible_python_interpreter: $(command -v python3)
    mash_probe: $scratch/probe
    mash_builds:
      test:
        - [sh, -c, 'echo build >> $scratch/actions']
  children:
    mash_api:
      hosts:
        api:
          mash_platform: test
          mash_install: [[sh, -c, 'echo api-install >> $scratch/actions']]
          mash_verify: [[sh, -c, 'echo api-verify >> $scratch/actions; test ! -e $scratch/fail-api']]
    mash_history:
      hosts:
        one:
          mash_platform: test
          mash_requires: [api]
          mash_install: [[sh, -c, 'echo one-install >> $scratch/actions; if test -e $scratch/upgrade; then echo "local index upgraded;"; fi']]
          mash_mcp: [unused]
        offline:
          mash_platform: test
          mash_preflight: [/usr/bin/false]
          mash_requires: [api]
          mash_install: [[/usr/bin/false]]
          mash_mcp: [unused]
INVENTORY
run_playbook() {
    local operation=$1 selection=$2 playbook=$3 job="$scratch/$1-$4"
    mkdir -p "$job/env"
    chmod 700 "$job" "$job/env"
    jq -n --arg operation "$operation" --arg repository "$repository" --arg job "$job" --argjson nodes "$selection" \
        '{mash_operation:$operation,mash_repository:$repository,mash_job_directory:$job,mash_selected_nodes:$nodes}' > "$job/env/extravars"
    uv run --project "$repository/deployment" --locked --no-sync ansible-runner run "$job" \
        --project-dir "$repository" --inventory "$scratch/inventory.yml" --playbook "$playbook" --ident job --json > "$job/runner.log" 2>&1
}
run_playbook deploy '["one","offline","api"]' deployment/playbooks/deploy.yml successful
[[ $(rg -c '^build$' "$scratch/actions") == 1 ]]
[[ $(jq -r .state "$scratch/deploy-successful/offline.json") == skipped ]]
[[ $(jq -r .state "$scratch/deploy-successful/one.json") == succeeded ]]
api_line=$(rg -n '^api-install$' "$scratch/actions" | cut -d: -f1)
node_line=$(rg -n '^one-install$' "$scratch/actions" | cut -d: -f1)
[[ "$api_line" -lt "$node_line" ]]
[[ $(rg -c '^check$' "$scratch/actions") == 1 ]]
if rg -q '^(sync|local-views)$' "$scratch/actions"; then echo 'Ordinary deployment ingested history.' >&2; exit 1; fi
before=$(rg -c '^one-install$' "$scratch/actions")
touch "$scratch/fail-api"
if run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml failed; then echo 'API failure was accepted.' >&2; exit 1; fi
[[ $(rg -c '^one-install$' "$scratch/actions") == "$before" ]]
rm "$scratch/fail-api"
touch "$scratch/upgrade"
run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml upgraded
[[ $(rg -c '^sync$' "$scratch/actions") == 1 ]]
if rg -q '^local-views$' "$scratch/actions"; then echo 'Upgrade replayed views before refreshing interpretation.' >&2; exit 1; fi
rm "$scratch/upgrade"
run_playbook sync '["one"]' deployment/playbooks/refresh.yml explicit
[[ $(rg -c '^sync$' "$scratch/actions") == 2 ]]
run_playbook local-views '["one"]' deployment/playbooks/refresh.yml offline
[[ $(rg -c '^local-views$' "$scratch/actions") == 1 ]]
if run_playbook sync '["api"]' deployment/playbooks/refresh.yml invalid; then echo 'API refresh was accepted.' >&2; exit 1; fi
MASH_DEPLOY_INVENTORY="$scratch/inventory.yml" MASH_DEPLOY_LOGS="$scratch/launcher" \
    bash "$repository/tools/fleet.sh" check api one offline > "$scratch/summary.jsonl"
jq -se 'length == 3 and any(.[]; .target == "one" and .state == "succeeded") and any(.[]; .target == "offline" and .state == "skipped")' \
    "$scratch/summary.jsonl" > /dev/null
if MASH_DEPLOY_INVENTORY="$scratch/inventory.yml" MASH_DEPLOY_LOGS="$scratch/launcher" \
    bash "$repository/tools/fleet.sh" deploy one > "$scratch/missing-api.jsonl" 2>/dev/null; then
    echo 'Direct launcher accepted a deployment without its API.' >&2; exit 1
fi
jq -se 'any(.[]; .state == "unverified")' "$scratch/missing-api.jsonl" > /dev/null
echo 'Ansible build reuse, ordering, skips, failure blocking and explicit refresh checks passed.'
