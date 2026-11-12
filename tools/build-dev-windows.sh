#!/usr/bin/env bash
# Build this checkout's revision on a Windows machine, where it will run. There
# is no cross-compiled Windows artifact: the machine builds its own, into a
# staging folder that install-dev-windows.sh then swaps in.
set -euo pipefail
[[ $# == 2 ]] || { echo 'usage: build-dev-windows.sh SSH_HOST REMOTE_CHECKOUT' >&2; exit 2; }
host=$1 checkout=$2
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$checkout" =~ ^[A-Za-z]:\\[A-Za-z0-9\\._-]+$ ]]
revision=$(git rev-parse HEAD)
[[ "$revision" =~ ^[0-9a-f]{40}$ ]]
transport=(-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=4)
script=$(cat <<POWERSHELL
\$ErrorActionPreference = 'Stop'
\$env:Path = "\$env:USERPROFILE\\.cargo\\bin;C:\\Program Files\\nodejs;\$env:APPDATA\\npm;C:\\Program Files\\Git\\cmd;\$env:Path"
foreach (\$tool in 'git', 'cargo', 'pnpm') {
    if (-not (Get-Command \$tool -ErrorAction SilentlyContinue)) { throw "build host has no \$tool" }
}
Set-Location '$checkout'
git fetch --quiet origin
if (\$LASTEXITCODE -ne 0) { throw 'git fetch failed' }
git checkout --quiet --detach $revision
if (\$LASTEXITCODE -ne 0) { throw 'the published history does not contain $revision; push it first' }
Set-Location dashboard
cmd /c "pnpm install --frozen-lockfile >NUL 2>&1"
if (\$LASTEXITCODE -ne 0) { throw 'pnpm install failed' }
cmd /c "pnpm build >NUL 2>&1"
if (\$LASTEXITCODE -ne 0) { throw 'dashboard build failed' }
Set-Location ..
\$env:MASH_BUILD_REVISION = '$revision'
cmd /c "cargo +stable build --release --locked --bin mash --bin mash-server >C:\\tools\\logs\\mash-build.log 2>&1"
if (\$LASTEXITCODE -ne 0) { throw 'cargo build failed; see C:\\tools\\logs\\mash-build.log' }
Set-Content -Path 'target\\release\\mash.revision' -Value '$revision'
POWERSHELL
)
# Encoded so the script crosses SSH and the remote shell without re-quoting.
encoded=$(printf '%s' "$script" | iconv -f UTF-8 -t UTF-16LE | base64 -w0)
ssh "${transport[@]}" "$host" "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
