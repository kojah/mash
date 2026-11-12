#!/usr/bin/env bash
# Swap the binaries built by build-dev-windows.sh into place on a Windows
# machine. Scheduled tasks that run Mash are stopped first, because Windows
# will not replace a running program, and started again afterwards.
set -euo pipefail
[[ $# -ge 3 ]] || { echo 'usage: install-dev-windows.sh SSH_HOST REMOTE_CHECKOUT INSTALL_DIR [TASK...]' >&2; exit 2; }
host=$1 checkout=$2 destination=$3
shift 3
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]
for path in "$checkout" "$destination"; do [[ "$path" =~ ^[A-Za-z]:\\[A-Za-z0-9\\._-]+$ ]]; done
tasks=''
for task in "$@"; do
    [[ "$task" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]
    tasks+="'$task',"
done
revision=$(git rev-parse HEAD)
transport=(-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=4)
script=$(cat <<POWERSHELL
\$ErrorActionPreference = 'Stop'
\$built = Join-Path '$checkout' 'target\\release'
if ((Get-Content (Join-Path \$built 'mash.revision') -ErrorAction SilentlyContinue) -ne '$revision') {
    throw 'the staged build is not this revision; build it first'
}
\$tasks = @(${tasks%,})
foreach (\$task in \$tasks) { Stop-ScheduledTask -TaskName \$task }
Start-Sleep -Seconds 3
Get-Process mash, mash-server -ErrorAction SilentlyContinue | Stop-Process -Force
try {
    foreach (\$name in 'mash.exe', 'mash-server.exe') {
        \$target = Join-Path '$destination' \$name
        if (Test-Path \$target) { Copy-Item -Force \$target "\$target.previous" }
        Copy-Item -Force (Join-Path \$built \$name) \$target
    }
} finally {
    foreach (\$task in \$tasks) { Start-ScheduledTask -TaskName \$task }
}
POWERSHELL
)
# Encoded so the script crosses SSH and the remote shell without re-quoting.
encoded=$(printf '%s' "$script" | iconv -f UTF-8 -t UTF-16LE | base64 -w0)
ssh "${transport[@]}" "$host" "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
