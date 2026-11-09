#!/usr/bin/env bash
# Exercise fleet policy with real Ansible and disposable, non-history targets.
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$repository/tools/controller-environment.sh"
# A fleet deploy runs this check inside an outer ansible-runner job. Its callback and cache settings (for example
# ANSIBLE_STDOUT_CALLBACK=awx_display) break every nested runner this script starts, so drop them first.
for inherited in $(compgen -e); do
    case $inherited in ANSIBLE_* | AWX_* | RUNNER_*) unset "$inherited" ;; esac
done
# A fleet-driven build passes its own probe executable; fixtures use their double.
unset MASH_FLEET_BINARY
scratch=$(mktemp -d "${TMPDIR:?}/ansible-fleet-test.XXXXXXXX")
# On an unexpected failure, show the newest nested runner log before the scratch tree is removed.
trap 'status=$?; if [[ $status -ne 0 ]]; then newest=$(ls -t "$scratch"/*/runner.log 2>/dev/null | head -n 1); [[ -n $newest ]] && tail -n 80 "$newest" >&2; fi; rm -rf "$scratch"' EXIT
cat > "$scratch/probe" <<'PROBE'
#!/usr/bin/env bash
set -euo pipefail
output='' mode='' all_history=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        --mode) mode=$2; shift 2 ;;
        --all-history) all_history=true; shift ;;
        *) shift ;;
    esac
done
if [[ "$mode" == local-views && -e "$TEST_ANSIBLE_UPGRADE" ]]; then exit 1; fi
if [[ -e "$TEST_ANSIBLE_UPGRADE" && "$mode" == sync ]]; then
    [[ "$all_history" == true ]]
else
    [[ "$all_history" == false ]]
fi
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
    mash_fleet: $scratch/probe
    mash_builds:
      test:
        - [sh, -c, 'echo build >> $scratch/actions']
  children:
    mash_api:
      hosts:
        api:
          mash_platform: test
          mash_preflight: [sh, -c, 'test ! -e $scratch/unavailable-api']
          mash_deploy_preflight: [[sh, -c, 'echo api-preflight >> $scratch/actions; test ! -e $scratch/fail-api']]
          mash_install: [[sh, -c, 'echo api-install >> $scratch/actions; if test -e $scratch/fail-after-api-install; then touch $scratch/fail-api; fi']]
          mash_verify: [[sh, -c, 'echo api-verify >> $scratch/actions; test ! -e $scratch/fail-api']]
          mash_replay_views: [[sh, -c, 'echo api-replay >> $scratch/actions; test ! -e $scratch/fail-replay']]
    mash_history:
      hosts:
        one:
          mash_platform: test
          mash_requires: [api]
          mash_deploy_preflight: [[sh, -c, 'test ! -e $scratch/deny-install']]
          mash_install:
            - [sh, -c, 'if test -e $scratch/fail-upgrade; then exit 1; fi; echo one-install >> $scratch/actions; if test -e $scratch/upgrade; then echo "local index upgraded;"; fi']
            - [sh, -c, 'echo one-binary-install >> $scratch/actions']
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
run_playbook preflight '["one","api"]' deployment/playbooks/preflight.yml ready
! grep -q '^build$' "$scratch/actions"
jq -e '.preflight == true and .deployed == false' "$scratch/preflight-ready/one.json" >/dev/null
touch "$scratch/fail-api"
if run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml stale-api; then
    echo 'Failed API preflight was accepted.' >&2; exit 1
fi
! grep -q '^build$' "$scratch/actions"
[[ $(jq -r .state "$scratch/deploy-stale-api/api.json") == failed ]]
rm -f "$scratch/fail-api"
touch "$scratch/deny-install"
if run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml denied-access; then
    echo 'Failed install-access check was accepted.' >&2; exit 1
fi
! grep -q '^build$' "$scratch/actions"
rm -f "$scratch/deny-install"
touch "$scratch/unavailable-api"
if run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml unavailable-api; then
    echo 'Unavailable required API was accepted.' >&2; exit 1
fi
! grep -q '^build$' "$scratch/actions"
[[ $(jq -r .state "$scratch/deploy-unavailable-api/api.json") == skipped ]]
rm -f "$scratch/unavailable-api"
run_playbook deploy '["one","offline","api"]' deployment/playbooks/deploy.yml successful
[[ $(grep -c '^build$' "$scratch/actions") == 1 ]]
[[ $(jq -r .state "$scratch/deploy-successful/offline.json") == skipped ]]
[[ $(jq -r .state "$scratch/deploy-successful/one.json") == succeeded ]]
api_line=$(grep -n '^api-install$' "$scratch/actions" | cut -d: -f1)
node_line=$(grep -n '^one-install$' "$scratch/actions" | cut -d: -f1)
[[ "$api_line" -lt "$node_line" ]]
[[ $(grep -c '^check$' "$scratch/actions") == 1 ]]
if grep -qE '^(sync|local-views)$' "$scratch/actions"; then echo 'Ordinary deployment ingested history.' >&2; exit 1; fi
before=$(grep -c '^one-install$' "$scratch/actions")
builds=$(grep -c '^build$' "$scratch/actions")
touch "$scratch/fail-api"
if run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml failed; then echo 'API failure was accepted.' >&2; exit 1; fi
[[ $(grep -c '^one-install$' "$scratch/actions") == "$before" ]]
[[ $(grep -c '^build$' "$scratch/actions") == "$builds" ]]
rm "$scratch/fail-api"
touch "$scratch/fail-after-api-install"
if run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml failed-after-install; then
    echo 'Post-install API readiness failure was accepted.' >&2; exit 1
fi
[[ $(grep -c '^one-install$' "$scratch/actions") == "$before" ]]
rm -f "$scratch/fail-after-api-install" "$scratch/fail-api"
binary_installs=$(grep -c '^one-binary-install$' "$scratch/actions")
touch "$scratch/fail-upgrade"
if run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml failed-upgrade; then
    echo 'Failed upgrade was accepted.' >&2; exit 1
fi
[[ $(grep -c '^one-binary-install$' "$scratch/actions") == "$binary_installs" ]]
[[ $(grep -c '^one-install$' "$scratch/actions") == "$before" ]]
grep -qF 'Installation step 1 failed for one' "$scratch/deploy-failed-upgrade/runner.log"
rm "$scratch/fail-upgrade"
touch "$scratch/upgrade"
run_playbook deploy '["one","api"]' deployment/playbooks/deploy.yml upgraded
[[ $(grep -c '^sync$' "$scratch/actions") == 1 ]]
if grep -q '^local-views$' "$scratch/actions"; then echo 'Upgrade replayed views before refreshing interpretation.' >&2; exit 1; fi
rm "$scratch/upgrade"
run_playbook sync '["one"]' deployment/playbooks/refresh.yml explicit
[[ $(grep -c '^sync$' "$scratch/actions") == 2 ]]
run_playbook local-views '["one"]' deployment/playbooks/refresh.yml offline
[[ $(grep -c '^local-views$' "$scratch/actions") == 1 ]]
if run_playbook sync '["api"]' deployment/playbooks/refresh.yml invalid; then echo 'API refresh was accepted.' >&2; exit 1; fi
replay_builds=$(grep -c '^build$' "$scratch/actions")
run_playbook replay-views '["api"]' deployment/playbooks/replay-views.yml successful
jq -e '.state == "succeeded" and .replayed == true and .deployed == false' "$scratch/replay-views-successful/api.json" >/dev/null
[[ $(grep -c '^api-replay$' "$scratch/actions") == 1 ]]
[[ $(grep -c '^build$' "$scratch/actions") == "$replay_builds" ]]
touch "$scratch/fail-replay"
if run_playbook replay-views '["api"]' deployment/playbooks/replay-views.yml failed; then echo 'Failed replay was accepted.' >&2; exit 1; fi
jq -e '.state == "failed"' "$scratch/replay-views-failed/api.json" >/dev/null
rm "$scratch/fail-replay"
if run_playbook replay-views '["one"]' deployment/playbooks/replay-views.yml invalid; then echo 'History target accepted for API replay.' >&2; exit 1; fi
MASH_DEPLOY_INVENTORY="$scratch/inventory.yml" MASH_DEPLOY_LOGS="$scratch/launcher" \
    bash "$repository/tools/fleet.sh" check api one offline > "$scratch/summary.jsonl"
jq -se 'length == 3 and any(.[]; .target == "one" and .state == "succeeded") and any(.[]; .target == "offline" and .state == "skipped")' \
    "$scratch/summary.jsonl" > /dev/null
if MASH_DEPLOY_INVENTORY="$scratch/inventory.yml" MASH_DEPLOY_LOGS="$scratch/launcher" \
    bash "$repository/tools/fleet.sh" deploy one > "$scratch/missing-api.jsonl" 2>/dev/null; then
    echo 'Direct launcher accepted a deployment without its API.' >&2; exit 1
fi
jq -se 'any(.[]; .state == "unverified")' "$scratch/missing-api.jsonl" > /dev/null
MASH_FLEET_BINARY="$scratch/probe" MASH_DEPLOY_INVENTORY="$scratch/inventory.yml" MASH_DEPLOY_LOGS="$scratch/all-launcher" \
    bash "$repository/tools/fleet.sh" check --all > "$scratch/all.jsonl"
jq -se 'length == 3 and ([.[].target] | sort) == ["api", "offline", "one"]' "$scratch/all.jsonl" > /dev/null
MASH_DEPLOY_INVENTORY="$scratch/inventory.yml" MASH_DEPLOY_LOGS="$scratch/all-sync" \
    bash "$repository/tools/fleet.sh" sync --all > "$scratch/all-sync.jsonl"
jq -se 'length == 2 and all(.[]; .target != "api")' "$scratch/all-sync.jsonl" > /dev/null
echo 'Ansible build reuse, ordering, skips, failure blocking, target selection and explicit refresh checks passed.'
bash "$repository/tools/test-fleet-systemd.sh"
bash "$repository/tools/test-fleet-launchd.sh"
