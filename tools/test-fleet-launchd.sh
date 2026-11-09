#!/usr/bin/env bash
# Exercise launchd orchestration with a disposable supervisor double, never the host's services.
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$repository/tools/controller-environment.sh"
scratch=$(mktemp -d "${TMPDIR:?}/fleet-launchd.XXXXXXXX")
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/bin" "$scratch/home"
cat > "$scratch/bin/launchctl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
    print)
        [[ -f "$FAKE_LAUNCH_STATE" ]] || exit 113
        echo 'state = running'
        ;;
    bootstrap|kickstart)
        echo "$1" >> "$FAKE_LAUNCH_ACTIONS"
        touch "$FAKE_LAUNCH_STATE"
        ;;
    bootout)
        echo "$1" >> "$FAKE_LAUNCH_ACTIONS"
        rm "$FAKE_LAUNCH_STATE"
        ;;
    *) exit 2 ;;
esac
SH
chmod 755 "$scratch/bin/launchctl"
cat > "$scratch/test.yml" <<YAML
- hosts: localhost
  connection: local
  gather_facts: false
  environment:
    PATH: "$scratch/bin:$PATH"
    FAKE_LAUNCH_STATE: "$scratch/loaded"
    FAKE_LAUNCH_ACTIONS: "$scratch/actions"
  vars:
    ansible_facts:
      user_dir: "$scratch/home"
      user_uid: 501
    mash_watch_launcher: /opt/mash/watch
  tasks:
    - ansible.builtin.include_tasks: "$repository/deployment/playbooks/tasks/watch-darwin.yml"
      loop: [watch-stop, watch-start, watch-start, watch-status, watch-stop, watch-stop]
      loop_control:
        loop_var: mash_operation
YAML
uv run --project "$repository/deployment" --locked --no-sync ansible-playbook \
    -i localhost, "$scratch/test.yml" > "$scratch/runner.log" 2>&1 || {
        tail -80 "$scratch/runner.log" >&2
        exit 1
    }
[[ $(cat "$scratch/actions") == $'bootstrap\nkickstart\nbootout' ]]
[[ ! -e "$scratch/loaded" ]]
[[ ! -e "$scratch/home/Library/LaunchAgents/dev.mash.index-watch.plist" ]]
echo 'Launchd start/stop idempotency and login-startup removal passed (supervisor double).'
