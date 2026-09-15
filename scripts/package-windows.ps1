<#
.SYNOPSIS
    构建 webdav-drive 的 Windows 免安装目录（不注册计划任务、不写注册表）。

.DESCRIPTION
    产物：dist\WebDavDrive-<版本>\
        drive.exe          主程序（Tauri 托盘）
        drive-pwcmd.exe    rclone --password-command 工具（必须与主程序同目录）
        rclone.exe         引擎（找不到会直接报错；确实不需要时用 -SkipRclone 跳过）
        LICENSE             WebDAV Drive 的 MIT 许可
        THIRD_PARTY_NOTICES.md  第三方项目与许可声明
        使用说明.txt        运行前置、数据目录、许可提示

    WinFsp 是 rclone 挂载的系统前置，必须由用户单独安装，绝不打进产物。
    WebDAV Drive 采用 MIT License；第三方组件继续适用各自许可。
#>
[CmdletBinding()]
param(
    [string]$RclonePath = '',
    [switch]$SkipRclone,
    [switch]$Zip
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root 'target\release'

Write-Host '==> cargo build --release' -ForegroundColor Cyan
cargo build --offline --release -p drive -p drive-core --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（$LASTEXITCODE）" }

$versionLine = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' |
    Select-Object -First 1
$version = if ($versionLine) { $versionLine.Matches[0].Groups[1].Value } else { '0.0.0' }

$stage = Join-Path $root "dist\WebDavDrive-$version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

foreach ($name in @('drive.exe', 'drive-pwcmd.exe')) {
    $source = Join-Path $release $name
    if (-not (Test-Path $source)) { throw "缺少构建产物：$source" }
    Copy-Item $source $stage -Force
    Write-Host "    + $name" -ForegroundColor Green
}

foreach ($name in @('LICENSE', 'THIRD_PARTY_NOTICES.md')) {
    $source = Join-Path $root $name
    if (-not (Test-Path $source)) { throw "缺少许可文件：$source" }
    Copy-Item $source $stage -Force
    Write-Host "    + $name" -ForegroundColor Green
}

if ($SkipRclone) {
    Write-Warning '按 -SkipRclone 跳过引擎；用户需自行提供 rclone.exe（或 RCLONE_EXE）。'
} else {
    $candidates = @()
    if ($RclonePath) { $candidates += $RclonePath }
    elseif ($env:RCLONE_EXE) { $candidates += $env:RCLONE_EXE }
    $candidates += (Join-Path $root 'bin\rclone.exe')
    # 开发机便利：与 drive-core 集成测试相同的旧仓库引擎位置（见 tests/rclone_provider.rs）
    $candidates += 'Z:\AI\webdav-drive\bin\rclone.exe'
    $engine = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $engine) {
        throw ('未找到 rclone.exe：请用 -RclonePath 指定，或设置 RCLONE_EXE，' +
            '或放到 bin\rclone.exe；确实要不含引擎的产物时显式加 -SkipRclone。')
    }
    Copy-Item $engine (Join-Path $stage 'rclone.exe') -Force
    $engineVersion = & $engine version 2>$null | Select-Object -First 1
    Write-Host "    + rclone.exe（$engineVersion）" -ForegroundColor Green
}

$engineNote = if ($SkipRclone) {
    'rclone.exe 未随包提供，请放到本目录或设置 RCLONE_EXE。'
} else {
    'rclone.exe 已随包提供（MIT）；许可文本见 THIRD_PARTY_NOTICES.md。'
}

$readme = @"
WebDAV Drive $version —— 免安装版

运行前置
  1. 安装 WinFsp（rclone 在 Windows 上挂载的硬依赖）：https://winfsp.dev/rel/
     注意：WinFsp 是 GPLv3 + FLOSS 例外 / 商业授权双轨；闭源商业分发需购买商业授权。
  2. $engineNote

使用
  1. 双击 drive.exe（托盘运行，关闭主窗口只是隐藏）。
  2. 新建连接 → 填 WebDAV 地址/账号 → 保存 → 测试连接 → 挂载。
  3. 需要无人值守时在「开机自动挂载」里注册（需要管理员权限）。
  4. 完全退出请使用托盘「退出并卸载所有驱动器」；程序会先确认全部挂载消失再退出。

数据目录
  %PROGRAMDATA%\WebDavDrive
    profiles.json       连接配置（密码经机器范围 DPAPI 保护）
    rclone.conf         引擎配置（静态加密，密钥为 config_key.enc）
    config_key.enc      配置密钥（DPAPI）
    agent.log           运行日志

安全提示
  机器范围 DPAPI 意味着本机任意用户都能解密；程序启动时会收紧数据目录 ACL，
  日志里会记录结果。多用户机器的完整隔离需要外部密钥库。

开源许可
  WebDAV Drive：MIT，见 LICENSE。
  第三方组件：见 THIRD_PARTY_NOTICES.md。WinFsp 由用户另行安装，不包含在本发布包内。
"@
$readme | Set-Content -Path (Join-Path $stage '使用说明.txt') -Encoding UTF8

if ($Zip) {
    $zipPath = Join-Path $root "dist\WebDavDrive-$version.zip"
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zipPath
    Write-Host "==> 已打包 $zipPath" -ForegroundColor Cyan
}

Write-Host "==> 产物目录：$stage" -ForegroundColor Cyan
Write-Host '==> WinFsp 未包含在发布包中；本脚本不会注册计划任务。' -ForegroundColor Yellow
