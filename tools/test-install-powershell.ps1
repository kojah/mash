# Offline installer checks: fake release assets, real checksums and file replacement.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../install.ps1')

$root = Join-Path ([IO.Path]::GetTempPath()) ('mash-installer-test-' + [Guid]::NewGuid().ToString('N'))
$originalOs = $env:OS
$originalArchitecture = $env:PROCESSOR_ARCHITECTURE
$originalWowArchitecture = $env:PROCESSOR_ARCHITEW6432
$script:mode = 'valid'
$script:requests = @()

function Invoke-WebRequest {
    param([switch] $UseBasicParsing, [string] $Uri, [string] $OutFile)
    $script:requests += $Uri
    if ($script:mode -eq 'network-failure') { throw 'fixture download failed' }
    $asset = if ($Uri.EndsWith('/SHA256SUMS')) { 'SHA256SUMS' } else { 'release.zip' }
    Copy-Item -LiteralPath (Join-Path $root $asset) -Destination $OutFile
}

function Assert-Equal($Actual, $Expected, [string] $Label) {
    if ($Actual -ne $Expected) { throw "$Label : expected '$Expected', got '$Actual'" }
}

function Assert-Fails([scriptblock] $Action, [string] $Message) {
    try { & $Action } catch {
        if ($_.Exception.Message -notlike "*$Message*") { throw }
        return
    }
    throw "Expected failure containing: $Message"
}

try {
    $env:OS = 'Windows_NT'
    $env:PROCESSOR_ARCHITECTURE = 'AMD64'
    $env:PROCESSOR_ARCHITEW6432 = ''
    New-Item -ItemType Directory -Path $root | Out-Null
    $fixture = Join-Path $root 'mash.exe'
    $archive = Join-Path $root 'release.zip'
    $checksums = Join-Path $root 'SHA256SUMS'
    $destination = Join-Path $root 'install with spaces'
    $installed = Join-Path $destination 'mash.exe'
    Set-Content -LiteralPath $fixture -Value 'new binary' -NoNewline
    $serverFixture = Join-Path $root 'mash-server.exe'
    Set-Content -LiteralPath $serverFixture -Value 'new server' -NoNewline
    Compress-Archive -LiteralPath @($fixture, $serverFixture) -DestinationPath $archive
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    $validChecksum = "$hash  mash-x86_64-pc-windows-msvc.zip"
    Set-Content -LiteralPath $checksums -Value $validChecksum

    Install-Mash -Directory $destination -Version 'v0.8.0'
    Assert-Equal (Get-Content -LiteralPath $installed -Raw) 'new binary' 'fresh install'
    Assert-Equal (Get-Content -LiteralPath (Join-Path $destination 'mash-server.exe') -Raw) 'new server' 'server install'
    Assert-Equal $script:requests[0] 'https://github.com/mashium/mash/releases/download/v0.8.0/mash-x86_64-pc-windows-msvc.zip' 'pinned release URL'

    Set-Content -LiteralPath $installed -Value 'previous binary' -NoNewline
    Set-Content -LiteralPath $checksums -Value (('0' * 64) + '  mash-x86_64-pc-windows-msvc.zip')
    Assert-Fails { Install-Mash -Directory $destination } 'checksum mismatch'
    Assert-Equal (Get-Content -LiteralPath $installed -Raw) 'previous binary' 'bad checksum preserves existing binary'

    Set-Content -LiteralPath $checksums -Value ($validChecksum + "`n" + $validChecksum)
    Assert-Fails { Install-Mash -Directory $destination } 'exactly one entry'
    Set-Content -LiteralPath $checksums -Value ($validChecksum.Replace('windows-msvc', 'other-target'))
    Assert-Fails { Install-Mash -Directory $destination } 'exactly one entry'

    Set-Content -LiteralPath $checksums -Value $validChecksum
    $script:mode = 'network-failure'
    Assert-Fails { Install-Mash -Directory $destination } 'fixture download failed'
    Assert-Equal (Get-Content -LiteralPath $installed -Raw) 'previous binary' 'download failure preserves existing binary'
    $script:mode = 'valid'

    $script:requests = @()
    Install-Mash -Directory $destination -Version ''
    Assert-Equal (Get-Content -LiteralPath $installed -Raw) 'new binary' 'replacement install'
    Assert-Equal $script:requests[0] 'https://github.com/mashium/mash/releases/latest/download/mash-x86_64-pc-windows-msvc.zip' 'latest release URL'
    Assert-Equal @(Get-ChildItem -LiteralPath $destination -Force -Filter '.mash-incoming-*').Count 0 'staging cleanup'

    Remove-Item -LiteralPath $archive
    $wrongFile = Join-Path $root 'README.txt'
    Set-Content -LiteralPath $wrongFile -Value 'not an executable'
    Compress-Archive -LiteralPath $wrongFile -DestinationPath $archive
    $wrongHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    Set-Content -LiteralPath $checksums -Value "$wrongHash  mash-x86_64-pc-windows-msvc.zip"
    Assert-Fails { Install-Mash -Directory $destination } 'contains no mash.exe'
    Assert-Equal (Get-Content -LiteralPath $installed -Raw) 'new binary' 'incomplete archive preserves existing binary'

    Assert-Fails { Install-Mash -Directory $destination -Version '../invalid' } 'release tag'
    $env:PROCESSOR_ARCHITECTURE = 'ARM64'
    Assert-Fails { Install-Mash -Directory $destination } 'No prebuilt Mash binary'
    $env:OS = 'Unix'
    Assert-Fails { Install-Mash -Directory $destination } 'supports Windows x64'
    Write-Host 'PowerShell installer checks passed.'
} finally {
    $env:OS = $originalOs
    $env:PROCESSOR_ARCHITECTURE = $originalArchitecture
    $env:PROCESSOR_ARCHITEW6432 = $originalWowArchitecture
    Remove-Item -LiteralPath $root -Recurse -Force
}
