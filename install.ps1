# Install the latest Windows x64 release. MASH_VERSION pins a release;
# MASH_INSTALL_DIR overrides the default $HOME\.local\bin directory.
$ErrorActionPreference = 'Stop'

function Install-Mash {
    param(
        [string] $Directory = $(if ($env:MASH_INSTALL_DIR) { $env:MASH_INSTALL_DIR } else { Join-Path $HOME '.local\bin' }),
        [string] $Version = $env:MASH_VERSION
    )

    if ($env:OS -ne 'Windows_NT') { throw 'This installer supports Windows x64. Use install.sh on macOS or Linux.' }
    $architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($architecture -ne 'AMD64') { throw "No prebuilt Mash binary for Windows $architecture; build from source." }
    if ($Version -and $Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?$') {
        throw 'MASH_VERSION must be a release tag such as v0.8.0.'
    }

    $base = 'https://github.com/mashium/mash/releases/latest/download'
    if ($Version) { $base = "https://github.com/mashium/mash/releases/download/$Version" }
    $archive = 'mash-x86_64-pc-windows-msvc.zip'
    $scratch = Join-Path ([IO.Path]::GetTempPath()) ('mash-install-' + [Guid]::NewGuid().ToString('N'))
    $incoming = $null
    $securityProtocol = [Net.ServicePointManager]::SecurityProtocol
    try {
        [Net.ServicePointManager]::SecurityProtocol = $securityProtocol -bor [Net.SecurityProtocolType]::Tls12
        New-Item -ItemType Directory -Path $scratch | Out-Null
        $download = Join-Path $scratch $archive
        $checksums = Join-Path $scratch 'SHA256SUMS'
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive" -OutFile $download
        Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $checksums
        $pattern = '^([a-fA-F0-9]{64})\s+\*?' + [Regex]::Escape($archive) + '$'
        $matchesForArchive = @(Get-Content -LiteralPath $checksums | Where-Object { $_ -match $pattern })
        if ($matchesForArchive.Count -ne 1) { throw 'Release checksums must contain exactly one entry for the Windows archive.' }
        $expected = [Regex]::Match($matchesForArchive[0], $pattern).Groups[1].Value
        if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash -ne $expected) {
            throw 'Mash archive checksum mismatch; existing installation was not changed.'
        }

        Expand-Archive -LiteralPath $download -DestinationPath (Join-Path $scratch 'unpacked')
        foreach ($name in @('mash.exe', 'mash-server.exe')) {
            $binary = Join-Path $scratch "unpacked\$name"
            if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "The release archive contains no $name." }
        }
        $Directory = [IO.Path]::GetFullPath($Directory)
        New-Item -ItemType Directory -Path $Directory -Force | Out-Null
        foreach ($name in @('mash.exe', 'mash-server.exe')) {
            $binary = Join-Path $scratch "unpacked\$name"
            $destination = Join-Path $Directory $name
            $incoming = Join-Path $Directory ('.mash-incoming-' + [Guid]::NewGuid().ToString('N') + '.exe')
            Copy-Item -LiteralPath $binary -Destination $incoming
            # Stage beside the destination so replacement stays on one filesystem.
            if (Test-Path -LiteralPath $destination) {
                [IO.File]::Replace($incoming, $destination, [NullString]::Value)
            } else {
                [IO.File]::Move($incoming, $destination)
            }
            Write-Host "Installed $destination"
        }
        if ($Directory -notin ($env:PATH -split ';')) { Write-Host "Add $Directory to your user PATH, then open a new terminal." }
        Write-Host 'Next: run mash qdrant install, then mash init. Reconnect existing Mash MCP sessions after an upgrade.'
    } finally {
        [Net.ServicePointManager]::SecurityProtocol = $securityProtocol
        if ($incoming -and (Test-Path -LiteralPath $incoming)) { Remove-Item -LiteralPath $incoming -Force }
        if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    }
}

# Dot-sourcing exposes the function for offline fixture tests without installing.
if ($MyInvocation.InvocationName -ne '.') { Install-Mash }
