<# Build the React/Vite assets embedded by the Tauri host. #>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$uiRoot = Join-Path (Split-Path -Parent $PSScriptRoot) 'apps\drive\ui'
$package = Join-Path $uiRoot 'package.json'
$modules = Join-Path $uiRoot 'node_modules'

if (-not (Test-Path -LiteralPath $package -PathType Leaf)) {
    throw "Missing UI package manifest: $package"
}

Push-Location $uiRoot
try {
    if (-not (Test-Path -LiteralPath $modules -PathType Container)) {
        Write-Host '==> Installing locked UI dependencies' -ForegroundColor Cyan
        npm ci --no-audit --no-fund
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed with exit code $LASTEXITCODE" }
    }

    Write-Host '==> Building React UI' -ForegroundColor Cyan
    npm run build
    if ($LASTEXITCODE -ne 0) { throw "UI build failed with exit code $LASTEXITCODE" }
}
finally {
    Pop-Location
}
