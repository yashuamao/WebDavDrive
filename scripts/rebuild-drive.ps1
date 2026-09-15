# rebuild-drive.ps1
# ASCII only on purpose (Windows PowerShell 5.1 + BOM-less UTF-8 breaks on Chinese).
#
# Rebuild webdav-drive with the console-flash fix and repackage dist.
# Must build BOTH drive and drive-core: drive-pwcmd.exe lives in drive-core,
# and an old drive-pwcmd.exe would still flash a console window.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\rebuild-drive.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\rebuild-drive.ps1 -SkipTests
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\rebuild-drive.ps1 -RclonePath D:\tools\rclone.exe

[CmdletBinding()]
param(
    [switch]$SkipTests,
    [string]$RclonePath = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    & (Join-Path $PSScriptRoot 'build-drive-ui.ps1')
    if ($LASTEXITCODE -ne 0) { throw "UI build failed with exit code $LASTEXITCODE" }

    Write-Host '==> 1/3 Stop old instances (release file locks)' -ForegroundColor Cyan
    Get-Process drive -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Get-Process drive-pwcmd -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1

    if (-not $SkipTests) {
        Write-Host '==> 2/3 Regression tests (cargo test --offline --workspace)' -ForegroundColor Cyan
        cargo test --offline --workspace -- --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw "tests failed with exit code $LASTEXITCODE" }
    } else {
        Write-Host '==> 2/3 Tests skipped (-SkipTests)' -ForegroundColor Yellow
    }

    Write-Host '==> 3/3 Release build + package' -ForegroundColor Cyan
    cargo build --offline --release -p drive -p drive-core --features drive/custom-protocol
    if ($LASTEXITCODE -ne 0) { throw "build failed with exit code $LASTEXITCODE" }

    & (Join-Path $PSScriptRoot 'package-windows.ps1') -RclonePath $RclonePath -Zip
    if ($LASTEXITCODE -ne 0) { throw "package failed with exit code $LASTEXITCODE" }

    Write-Host ''
    Write-Host 'Done. Run the NEW artifact (do not reuse old shortcuts/paths):' -ForegroundColor Green
    $versionLine = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' |
        Select-Object -First 1
    $version = if ($versionLine) { $versionLine.Matches[0].Groups[1].Value } else { '0.0.0' }
    Write-Host "    $root\dist\WebDavDrive-$version\drive.exe" -ForegroundColor Green
} finally {
    Pop-Location
}
