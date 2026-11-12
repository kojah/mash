#!/usr/bin/env bash
# Install, start, stop or inspect the index watcher on a Windows machine. The
# watcher is the scheduled task `mash-index-watch`, which starts with the
# machine and runs whether or not anyone is signed in. Prints the task's state
# on the first line: `active` when it is running.
set -euo pipefail
[[ $# == 3 ]] || { echo 'usage: watch-windows.sh SSH_HOST OPERATION LAUNCHER' >&2; exit 2; }
host=$1 operation=$2 launcher=$3
[[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]
[[ "$operation" =~ ^watch-(start|stop|status|logs)$ ]]
[[ "$launcher" =~ ^[A-Za-z]:[\\/][A-Za-z0-9\\/._-]+$ ]]
transport=(-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=4)
script=$(cat <<POWERSHELL
\$ErrorActionPreference = 'Stop'
\$ProgressPreference = 'SilentlyContinue'
\$name = 'mash-index-watch'
\$launcher = '$launcher'
\$task = Get-ScheduledTask -TaskName \$name -ErrorAction SilentlyContinue
if ('$operation' -eq 'watch-start') {
    if (-not (Test-Path -LiteralPath \$launcher)) { throw 'the watcher launcher does not exist on this machine' }
    if (\$launcher.ToLower().EndsWith('.ps1')) {
        \$run = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ('-NoProfile -ExecutionPolicy Bypass -File "' + \$launcher + '"')
    } else {
        \$run = New-ScheduledTaskAction -Execute \$launcher
    }
    \$who = New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType S4U -RunLevel Highest
    \$keep = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
    # Registering again would restart a healthy watcher, so only a missing or
    # differently configured task is written.
    \$same = \$task -and \$task.Actions.Count -eq 1 -and \$task.Actions[0].Execute -eq \$run.Execute -and \$task.Actions[0].Arguments -eq \$run.Arguments
    if (-not \$same) {
        Register-ScheduledTask -TaskName \$name -Action \$run -Trigger (New-ScheduledTaskTrigger -AtStartup) -Principal \$who -Settings \$keep -Force | Out-Null
    }
    Enable-ScheduledTask -TaskName \$name | Out-Null
    if ((Get-ScheduledTask -TaskName \$name).State -ne 'Running') { Start-ScheduledTask -TaskName \$name }
    foreach (\$attempt in 1..10) {
        if ((Get-ScheduledTask -TaskName \$name).State -eq 'Running') { break }
        Start-Sleep -Seconds 1
    }
} elseif ('$operation' -eq 'watch-stop' -and \$task) {
    Stop-ScheduledTask -TaskName \$name
    Disable-ScheduledTask -TaskName \$name | Out-Null
}
\$task = Get-ScheduledTask -TaskName \$name -ErrorAction SilentlyContinue
if (\$task -and \$task.State -eq 'Running') { 'active' } elseif (\$task) { 'inactive' } else { 'not-installed' }
if ('$operation' -eq 'watch-logs' -and \$task) {
    \$info = Get-ScheduledTaskInfo -TaskName \$name
    'last run ' + \$info.LastRunTime.ToString('u') + ', result ' + \$info.LastTaskResult + ', state ' + \$task.State
}
POWERSHELL
)
# Encoded so the script crosses SSH and the remote shell without re-quoting.
encoded=$(printf '%s' "$script" | iconv -f UTF-8 -t UTF-16LE | base64 -w0)
ssh "${transport[@]}" "$host" "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
