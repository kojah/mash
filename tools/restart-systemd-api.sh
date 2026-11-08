#!/usr/bin/env bash
# Systemd owns the API process. Keep shared silver and transfer receipts together.
set -euo pipefail
umask 077
usage() {
    echo 'usage: restart-systemd-api.sh preflight|install|check UNIT ARTIFACT DESTINATION CONFIG ENV POINTER API_URL TOKEN_ENV LOG_ROOT SILVER_DIR [--retire-incompatible-silver] [--next-environment PATH] [--previous-binary PATH --next-unit PATH]' >&2
    exit 2
}
[[ $# -ge 11 ]] || usage
action=$1 unit=$2 artifact=$3 destination=$4 config=$5 environment=$6
pointer=$7 base=$8 token_name=$9 root=${10} silver=${11}
retire=false
next_environment=''
previous_binary=$destination
next_unit=''
shift 11
while [[ $# -gt 0 ]]; do
    case $1 in
        --retire-incompatible-silver) retire=true; shift ;;
        --next-environment) [[ $# -ge 2 && -r $2 ]] || usage; next_environment=$2; shift 2 ;;
        --previous-binary) [[ $# -ge 2 && $2 == /* && -x $2 ]] || usage; previous_binary=$2; shift 2 ;;
        --next-unit) [[ $# -ge 2 && $2 == /* && -r $2 ]] || usage; next_unit=$2; shift 2 ;;
        *) usage ;;
    esac
done
[[ $action == preflight || $action == install || $action == check ]] || usage
[[ $unit == *.service && $unit != */* && $destination == /* && $silver == /* && $root == /* ]]
source "$(dirname "$0")/dev-api-common.sh"
load_api_environment "$environment" "$token_name"
exec {rollout_lock}>"$pointer.deploy.lock"
flock -w 60 "$rollout_lock"
marker="$pointer.systemd-pending"
[[ ! -e $marker ]] || {
    echo "Unfinished systemd rollout: inspect $marker; do not restore old state after activation." >&2
    exit 1
}
mkdir -p "$root"
scratch=$(mktemp -d "$root/systemd-api.XXXXXXXX")
active_binary=$previous_binary
# On a completed split deployment, the configured old path may still be the CLI.
# Verify the running server against the destination before choosing its identity.
if [[ $previous_binary != "$destination" && -f $destination ]]; then
    running_pid=$(systemctl --user show "$unit" --property MainPID --value)
    if [[ $running_pid =~ ^[0-9]+$ && $running_pid -gt 1 ]]; then
        running_digest=$(sha256sum "/proc/$running_pid/exe")
        destination_digest=$(sha256sum "$destination")
        if [[ $(readlink "/proc/$running_pid/exe") == "$(realpath "$destination")" && ${running_digest%% *} == "${destination_digest%% *}" ]]; then
            active_binary=$destination
        fi
    fi
fi
unit_path=''
unit_unchanged=true
if [[ -n $next_unit ]]; then
    unit_path=$(systemctl --user show "$unit" --property FragmentPath --value)
    [[ $unit_path == /* && -f $unit_path && ! -L $unit_path && -w $unit_path && -w $(dirname "$unit_path") ]]
    cmp -s "$next_unit" "$unit_path" || unit_unchanged=false
elif [[ $previous_binary != "$destination" ]]; then
    echo 'Changing the server executable path requires --next-unit.' >&2
    exit 1
fi
check_service() {
    systemctl --user is-active --quiet "$unit" || return 1
    pid=$(systemctl --user show "$unit" --property MainPID --value) || return 1
    [[ $pid =~ ^[0-9]+$ && $pid -gt 1 ]] || return 1
    local expected actual
    expected=$(sha256sum "$active_binary") || return 1
    actual=$(sha256sum "/proc/$pid/exe") || return 1
    [[ ${expected%% *} == "${actual%% *}" ]] || return 1
    check_api_http "$base" "$scratch"
}
check_service
[[ $action != check ]] || exit 0
[[ ( ! -e $destination || -w $destination ) && -w $(dirname "$destination") && -w $pointer && -w $(dirname "$pointer") ]]
silver_schema_range "$(dirname "$0")/../app/storage/schema.rs"
if [[ $action == install ]]; then
    [[ -x $artifact ]]
    source_schema=$schema source_schema_min=$schema_min
    silver_binary_range "$artifact"
    [[ $schema == "$source_schema" && $schema_min == "$source_schema_min" ]] || {
        echo 'Candidate schema range does not match preflight source.' >&2; exit 1;
    }
fi
incompatible=false
shopt -s nullglob
for directory in "$silver" "$silver"/*; do
    [[ ! -d $directory || ( -r $directory && -x $directory ) ]] || exit 1
done
for file in "$silver"/*/*.sqlite3; do
    [[ $file != "$silver/sync/catalog.sqlite3" ]] || continue
    version=$(silver_version "$file")
    silver_schema_supported "$version" || incompatible=true
done
if $incompatible && ! $retire; then
    echo 'Shared silver is incompatible; explicitly plan full shared-state retirement and node republishing.' >&2
    exit 1
fi
if $incompatible; then
    [[ -d $silver && ! -L $silver && -w $(dirname "$silver") ]]
    case "$(realpath "$root")/" in "$(realpath "$silver")/"*)
        echo 'Retained backups must be outside shared silver.' >&2; exit 1 ;;
    esac
    [[ $(stat -c %d "$silver") == "$(stat -c %d "$root")" ]] || {
        echo 'Shared state and retained backup must be on the same filesystem.' >&2; exit 1;
    }
fi
[[ $action != preflight ]] || {
    printf 'Systemd preflight passed; shared-state retirement required: %s\n' "$incompatible"
    exit 0
}
# Repeat runtime verification before stopping anything.
check_service
digest=$(sha256sum "$artifact")
config_digest=$(sha256sum "$config")
environment_digest=$(sha256sum "${next_environment:-$environment}")
if ! $incompatible && $unit_unchanged && [[ $active_binary == "$destination" ]] && jq -e --arg binary "${digest%% *}" --arg config "${config_digest%% *}" \
    --arg environment "${environment_digest%% *}" \
    '.binary_sha256 == $binary and .config_sha256 == $config and .environment_sha256 == $environment' "$pointer" >/dev/null; then
    echo '{"deployed":false,"unchanged":true}'
    exit 0
fi
cp "$active_binary" "$scratch/previous-mash"
restore_binary=$active_binary
if [[ -n $unit_path ]]; then cp -p "$unit_path" "$scratch/previous-unit"; fi
cp "$pointer" "$scratch/previous-release.json"
cp "$environment" "$scratch/previous-environment"
publication_identity "$scratch/freshness.json" > "$scratch/before.json"
printf '%s\n' "$scratch" > "$marker"
printf 'prepared\n' > "$scratch/phase"
activation=false
stopped=false
finish() {
    result=$?
    trap - EXIT
    if [[ $result != 0 && $activation == false && $stopped == true ]]; then
        # No candidate has been started, so it cannot have acknowledged writes.
        if [[ -d $scratch/shared-state ]]; then
            [[ ! -e $silver ]] || rmdir "$silver"
            mv "$scratch/shared-state" "$silver"
        fi
        bash "$(dirname "$0")/install-dev-binary.sh" "$scratch/previous-mash" "$restore_binary"
        active_binary=$restore_binary
        if [[ -n $unit_path ]]; then
            cp -p "$scratch/previous-unit" "$unit_path.incoming.$$"
            mv "$unit_path.incoming.$$" "$unit_path"
            systemctl --user daemon-reload
        fi
        cp "$scratch/previous-environment" "$environment.incoming.$$"
        mv "$environment.incoming.$$" "$environment"
        load_api_environment "$environment" "$token_name"
        systemctl --user start "$unit"
        # Keep the marker if restoration or readiness could not be established.
        check_service
        jq --argjson pid "$pid" '.pid=$pid' "$scratch/previous-release.json" > "$pointer.incoming.$$"
        mv "$pointer.incoming.$$" "$pointer"
        printf 'restored-before-activation\n' > "$scratch/phase"
        rm "$marker"
    elif [[ $result != 0 ]]; then
        echo "Rollout incomplete; retained state: $scratch. After activation, recover forward; never automatically restore old acknowledged state." >&2
    fi
    exit "$result"
}
trap finish EXIT
systemctl --user stop "$unit"
[[ $(systemctl --user show "$unit" --property MainPID --value) == 0 ]]
stopped=true
printf 'stopped\n' > "$scratch/phase"
if $incompatible; then
    # One directory rename retains every node, WAL, receipt, spool piece and lock.
    # The old process is stopped; no server can acknowledge more writes there.
    mv "$silver" "$scratch/shared-state"
    mkdir -m 700 "$silver"
fi
bash "$(dirname "$0")/install-dev-binary.sh" "$artifact" "$destination"
active_binary=$destination
if [[ -n $unit_path ]]; then
    cp "$next_unit" "$unit_path.incoming.$$"
    chmod 600 "$unit_path.incoming.$$"
    mv "$unit_path.incoming.$$" "$unit_path"
    systemctl --user daemon-reload
fi
if [[ -n $next_environment ]]; then
    cp "$next_environment" "$environment.incoming.$$"
    chmod 600 "$environment.incoming.$$"
    mv "$environment.incoming.$$" "$environment"
    load_api_environment "$environment" "$token_name"
fi
# Persist the recovery boundary BEFORE starting a process that can accept writes.
printf 'activation-started\n' > "$scratch/phase"
sync -f "$scratch/phase"
activation=true
systemctl --user start "$unit"
ready=false
for ((attempt=0; attempt<60; attempt++)); do
    if check_service 2>"$scratch/readiness-error.log"; then ready=true; break; fi
    sleep 1
done
[[ $ready == true ]]
publication_identity "$scratch/freshness.json" > "$scratch/after.json"
if ! $incompatible; then cmp "$scratch/before.json" "$scratch/after.json"; fi
digest=$(sha256sum "$destination")
config_digest=$(sha256sum "$config")
environment_digest=$(sha256sum "$environment")
revision=$(git -C "$(dirname "$0")/.." rev-parse HEAD)
jq -n --arg source_commit "$revision" --arg binary_sha256 "${digest%% *}" \
    --arg config_sha256 "${config_digest%% *}" --arg environment_sha256 "${environment_digest%% *}" \
    --arg service "$unit" --argjson pid "$pid" --argjson republish "$incompatible" \
    --arg backup "$scratch" \
    '{source_commit:$source_commit,binary_sha256:$binary_sha256,config_sha256:$config_sha256,environment_sha256:$environment_sha256,service:$service,pid:$pid,deployed:true,republish_required:$republish,backup:$backup}' > "$pointer.incoming.$$"
mv "$pointer.incoming.$$" "$pointer"
printf 'complete\n' > "$scratch/phase"
rm "$marker"
jq '{source_commit,service,deployed,republish_required,backup}' "$pointer"
