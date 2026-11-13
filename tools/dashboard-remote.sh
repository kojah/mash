#!/usr/bin/env bash
# Make this machine's `mash dash` reachable from your other devices over
# Tailscale: a sign-in token, a user service that keeps the dashboard running,
# and an HTTPS address inside your tailnet. Nothing here is public: only
# devices signed in to your tailnet can open the address.
#
#   tools/dashboard-remote.sh            set up (or repair) and print the address
#   tools/dashboard-remote.sh --token    print the sign-in token again
#   tools/dashboard-remote.sh --qr       show the sign-in QR code again
#   tools/dashboard-remote.sh --remove   stop serving and remove the service
#
# Linux with systemd user services and Tailscale only. Elsewhere, follow
# docs/dashboard.md ("Open it from your phone or another computer").
set -euo pipefail

port=${MASH_DASH_PORT:-8790}
unit=mash-dash.service
config_dir=${XDG_CONFIG_HOME:-$HOME/.config}/mash
token_file=$config_dir/dashboard.env
unit_file=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$unit

need() {
    command -v "$1" >/dev/null || { echo "Required command is missing: $1" >&2; exit 2; }
}

# The sign-in link as a QR code for a phone camera, when a QR tool exists.
# The token rides in the fragment, which browsers never send to a server.
show_qr() {
    local link=$1
    if command -v qrencode >/dev/null; then
        qrencode -t ANSIUTF8 -m 1 "$link"
    elif command -v uv >/dev/null; then
        uv run --quiet --with segno python3 -c '
import sys, segno
segno.make(sys.argv[1], error="m").terminal(compact=True, border=1)' "$link" 2>/dev/null \
            || echo '(Install qrencode to show a QR code here.)'
    else
        echo '(Install qrencode to show a QR code here.)'
    fi
}

case ${1:-} in
    --token)
        [[ -r $token_file ]] || { echo 'No dashboard token yet; run this script without arguments first.' >&2; exit 1; }
        sed -n 's/^MASH_DASH_TOKEN=//p' "$token_file"
        exit 0
        ;;
    --remove)
        need systemctl
        systemctl --user disable --now "$unit" 2>/dev/null || true
        rm -f -- "$unit_file"
        systemctl --user daemon-reload
        if command -v tailscale >/dev/null; then
            tailscale serve --https="$port" off 2>/dev/null || true
        fi
        echo "Removed $unit and its tailnet address. The token file is kept: $token_file"
        exit 0
        ;;
    '' | --qr) ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
esac

for dependency in systemctl tailscale python3; do need "$dependency"; done
mash=$(command -v mash) || { echo 'mash is not on PATH; install it first.' >&2; exit 2; }

# The name other devices use for this machine, without the trailing dot.
name=$(tailscale status --json | python3 -c '
import json, sys
print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))')
[[ -n $name ]] || { echo 'Tailscale is not connected; run `tailscale up` first.' >&2; exit 1; }
host=$name:$port

if [[ ${1:-} == --qr ]]; then
    [[ -r $token_file ]] || { echo 'No dashboard token yet; run this script without arguments first.' >&2; exit 1; }
    show_qr "https://$host/#signin=$(sed -n 's/^MASH_DASH_TOKEN=//p' "$token_file")"
    exit 0
fi

mkdir -p "$config_dir" "$(dirname "$unit_file")"
if [[ ! -s $token_file ]]; then
    umask 077
    printf 'MASH_DASH_TOKEN=%s\n' "$(python3 -c 'import secrets; print(secrets.token_hex(24))')" >"$token_file"
fi
chmod 600 "$token_file"

cat >"$unit_file" <<EOF
[Unit]
Description=Mash dashboard for this tailnet
After=network-online.target

[Service]
EnvironmentFile=$token_file
ExecStart=$mash dash --listen 127.0.0.1:$port --allow-host $host --access-token-env MASH_DASH_TOKEN
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
EOF
systemctl --user daemon-reload
systemctl --user enable --quiet "$unit"
systemctl --user restart "$unit"

# HTTPS on the same port number inside the tailnet, forwarded to loopback.
tailscale serve --bg --https="$port" "http://127.0.0.1:$port" >/dev/null

for _ in 1 2 3 4 5 6 7 8 9 10; do
    if curl --silent --fail --max-time 2 "http://127.0.0.1:$port/api/health" -H "Host: 127.0.0.1:$port" >/dev/null; then
        token=$(sed -n 's/^MASH_DASH_TOKEN=//p' "$token_file")
        echo "Dashboard: https://$host/"
        echo "Sign-in token: $token"
        echo 'On a device signed in to your tailnet, scan this code, or open the address and enter the token once.'
        show_qr "https://$host/#signin=$token"
        exit 0
    fi
    sleep 1
done
echo "The dashboard did not start; see: journalctl --user -u $unit" >&2
exit 1
