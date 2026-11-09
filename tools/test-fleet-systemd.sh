#!/usr/bin/env bash
# Real Ansible modules against a disposable systemctl double; no host units touched.
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$repository/tools/controller-environment.sh"
scratch=$(mktemp -d "${TMPDIR:?}/fleet-systemd.XXXXXXXX")
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/bin" "$scratch/home"
cat > "$scratch/bin/systemctl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ ${1:-} == --user ]] && shift
case "$1" in
    show)
        active=inactive
        [[ ! -f "$FAKE_SYSTEMD_ROOT/running" ]] || active=active
        if [[ "$*" == *--value* ]]; then echo "$active"; else
            printf 'LoadState=loaded\nActiveState=%s\nUnitFileState=enabled\n' "$active"
        fi
        ;;
    is-enabled)
        if [[ -f "$FAKE_SYSTEMD_ROOT/enabled" ]]; then echo enabled; else echo disabled; exit 1; fi
        ;;
    enable) touch "$FAKE_SYSTEMD_ROOT/enabled" ;;
    disable) rm -f "$FAKE_SYSTEMD_ROOT/enabled" ;;
    start)
        echo start >> "$FAKE_SYSTEMD_ROOT/actions"
        touch "$FAKE_SYSTEMD_ROOT/running"
        ;;
    stop)
        echo stop >> "$FAKE_SYSTEMD_ROOT/actions"
        rm -f "$FAKE_SYSTEMD_ROOT/running"
        ;;
    daemon-reload) echo reload >> "$FAKE_SYSTEMD_ROOT/actions" ;;
    *) echo "Unexpected systemctl: $*" >&2; exit 2 ;;
esac
SH
chmod 755 "$scratch/bin/systemctl"
cat > "$scratch/test.yml" <<YAML
- hosts: localhost
  connection: local
  gather_facts: false
  environment:
    PATH: "$scratch/bin:$PATH"
    FAKE_SYSTEMD_ROOT: "$scratch"
  vars:
    ansible_facts:
      user_dir: "$scratch/home"
    mash_watch_launcher: /opt/mash/watch
  tasks:
    - ansible.builtin.include_tasks: "$repository/deployment/playbooks/tasks/watch-linux.yml"
      loop: [watch-stop, watch-start, watch-start, watch-status, watch-stop, watch-stop]
      loop_control:
        loop_var: mash_operation
YAML
uv run --project "$repository/deployment" --locked --no-sync ansible-playbook \
    -i localhost, "$scratch/test.yml" > "$scratch/runner.log" 2>&1 || {
        tail -80 "$scratch/runner.log" >&2
        exit 1
    }
[[ $(cat "$scratch/actions") == $'reload\nstart\nstop' ]]
[[ ! -e "$scratch/running" && ! -e "$scratch/enabled" ]]
grep -q '^Restart=on-failure$' "$scratch/home/.config/systemd/user/mash-index-watch.service"
echo 'Systemd start/stop idempotency and login-startup disabling passed (supervisor double).'
