# Diagnostic only: fresh storage on a disposable hosted Windows runner.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $IsWindows) {
    throw 'Requires a disposable Windows GitHub Actions runner.'
}
$root = Join-Path $env:RUNNER_TEMP 'qdrant-path-probe'
$reports = Join-Path $root 'reports'
New-Item -ItemType Directory -Path $reports -Force | Out-Null
$zip = Join-Path $root 'qdrant.zip'
Invoke-WebRequest 'https://github.com/qdrant/qdrant/releases/download/v1.20.0/qdrant-x86_64-pc-windows-msvc.zip' -OutFile $zip
Expand-Archive $zip (Join-Path $root 'bin')
$exe = Join-Path $root 'bin/qdrant.exe'
$results = @()
foreach ($mode in @('relative', 'absolute', 'short-absolute')) {
    $work = Join-Path $HOME "qdrant-path-probe-$($env:GITHUB_RUN_ID)-$mode"
    $storageRoot = if ($mode -eq 'short-absolute') { "C:\qp-$($env:GITHUB_RUN_ID)" } else { $work }
    New-Item -ItemType Directory -Path $work | Out-Null
    $env:QDRANT__SERVICE__HOST = '127.0.0.1'
    $env:QDRANT__STORAGE__STORAGE_PATH = if ($mode -eq 'relative') { './storage' } else { Join-Path $storageRoot 'storage' }
    $env:QDRANT__STORAGE__SNAPSHOTS_PATH = if ($mode -eq 'relative') { './snapshots' } else { Join-Path $storageRoot 'snapshots' }
    $process = Start-Process $exe -WorkingDirectory $work -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $reports "$mode.stdout.log") -RedirectStandardError (Join-Path $reports "$mode.stderr.log")
    $result = [ordered]@{ mode = $mode; storage = $env:QDRANT__STORAGE__STORAGE_PATH; ready = $false; indexes = @(); error = $null }
    try {
        for ($i = 0; $i -lt 30; $i++) {
            try {
                Invoke-RestMethod 'http://127.0.0.1:6333/readyz' -TimeoutSec 2 | Out-Null
                $result.ready = $true
                break
            } catch { Start-Sleep -Seconds 1 }
        }
        if (-not $result.ready) { throw 'Qdrant never became ready' }
        # Match Mash's long state-collection name and empty-vector payload indexes.
        $collection = 'mash_views_' + ('a' * 64) + '_state'
        $url = "http://127.0.0.1:6333/collections/$collection"
        Invoke-RestMethod $url -Method Put -ContentType 'application/json' -Body '{"vectors":{},"on_disk_payload":true}' | Out-Null
        foreach ($field in @('version', 'node_id', 'kind')) {
            $body = @{ field_name = $field; field_schema = 'keyword' } | ConvertTo-Json -Compress
            Invoke-RestMethod "$url/index?wait=true" -Method Put -ContentType 'application/json' -Body $body | Out-Null
            $result.indexes += $field
        }
    } catch {
        $result.error = $_.ToString()
    } finally {
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id
            $process.WaitForExit()
        }
        $results += [pscustomobject]$result
        $results | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $reports 'results.json')
    }
}
$results | Format-Table mode, ready, indexes, error -Wrap
