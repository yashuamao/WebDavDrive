<#
.SYNOPSIS
    Build the NSIS installer for WebDAV Drive.
.DESCRIPTION
    ASCII only on purpose (Windows PowerShell 5.1 + BOM-less UTF-8 breaks on Chinese).
    Requires the local Tauri CLI; fails with a clear message when it is missing.
    Output: target\release\bundle\nsis\WebDAV Drive_<version>_x64-setup.exe
#>
[CmdletBinding()]
param([switch]$SkipUiBuild, [string]$RclonePath = '')

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$ui = Join-Path $root 'apps\drive\ui'

if (-not (Test-Path (Join-Path $ui 'node_modules\.bin\tauri.cmd'))) {
    throw "Tauri CLI not found. Run once (needs network): cd apps/drive/ui ; npm install --save-dev @tauri-apps/cli@^2"
}

if (-not $SkipUiBuild) {
    & (Join-Path $PSScriptRoot 'build-drive-ui.ps1')
    if ($LASTEXITCODE -ne 0) { throw "UI build failed with exit code $LASTEXITCODE" }
}

# Stage bundle resources: rclone engine, password helper, license files.
$resDir = Join-Path $root 'apps\drive\src-tauri\resources'
New-Item -ItemType Directory -Force -Path $resDir | Out-Null

cargo build --offline --release -p drive-core --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw "drive-pwcmd build failed with exit code $LASTEXITCODE" }
Copy-Item (Join-Path $root 'target\release\drive-pwcmd.exe') $resDir -Force

$engine = @($RclonePath, $env:RCLONE_EXE, (Join-Path $root 'bin\rclone.exe'), 'Z:\AI\webdav-drive\bin\rclone.exe') |
    Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $engine) { throw "rclone.exe not found. Pass -RclonePath or set RCLONE_EXE." }
Copy-Item $engine (Join-Path $resDir 'rclone.exe') -Force

foreach ($name in @('LICENSE', 'THIRD_PARTY_NOTICES.md')) {
    Copy-Item (Join-Path $root $name) $resDir -Force
}
Write-Host ("==> staged resources: " + ((Get-ChildItem $resDir | ForEach-Object { $_.Name }) -join ', ')) -ForegroundColor Cyan

Push-Location (Join-Path $root 'apps\drive\src-tauri')
try {
    & (Join-Path $ui 'node_modules\.bin\tauri.cmd') build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

$nsisDir = Join-Path $root 'target\release\bundle\nsis'
if (-not (Test-Path $nsisDir)) { throw "NSIS output directory not found: $nsisDir" }
Write-Host '==> NSIS installer:' -ForegroundColor Cyan
Get-ChildItem $nsisDir -Filter '*.exe' | ForEach-Object {
    Write-Host ("    " + $_.Name + "  " + $_.Length + ' bytes')
    Write-Host ("    SHA-256 " + (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())
}
