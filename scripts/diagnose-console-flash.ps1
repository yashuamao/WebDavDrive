# diagnose-console-flash.ps1
# ASCII only on purpose: Windows PowerShell 5.1 reads BOM-less UTF-8 as ANSI and
# Chinese characters would break parsing.
#
# Purpose: catch console processes that flash during webdav-drive operation,
# and print their parent process so we know who spawns them.
#
# Run (elevated PowerShell is recommended):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\diagnose-console-flash.ps1
#
# Keep webdav-drive running and reproduce the flashing while this script runs.

[CmdletBinding()]
param(
    [int]$Seconds = 90
)

$filter = "Name='conhost.exe' OR Name='cmd.exe' OR Name='schtasks.exe' OR Name='icacls.exe' OR Name='rclone.exe' OR Name='drive-pwcmd.exe' OR Name='drive.exe'"

Write-Host "Watching for $Seconds seconds. Reproduce the flashing now..." -ForegroundColor Cyan

$seen = @{}
Get-CimInstance Win32_Process -Filter $filter -ErrorAction SilentlyContinue |
    ForEach-Object { $seen[[int]$_.ProcessId] = $true }

$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
    foreach ($p in (Get-CimInstance Win32_Process -Filter $filter -ErrorAction SilentlyContinue)) {
        $procId = [int]$p.ProcessId
        if (-not $seen.ContainsKey($procId)) {
            $seen[$procId] = $true
            $parentName = '?'
            try {
                $parent = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.ParentProcessId)" -ErrorAction Stop
                if ($parent) { $parentName = $parent.Name }
            } catch { }
            $stamp = Get-Date -Format 'HH:mm:ss.fff'
            Write-Host ("{0}  {1} (pid {2})  <-  {3} (pid {4})" -f $stamp, $p.Name, $procId, $parentName, $p.ParentProcessId) -ForegroundColor Yellow
            if ($p.CommandLine) {
                Write-Host ("           " + $p.CommandLine) -ForegroundColor DarkGray
            }
        }
    }
    Start-Sleep -Milliseconds 100
}

Write-Host "Done. Send the lines above to the assistant." -ForegroundColor Cyan
Write-Host "Typical readings:" -ForegroundColor Gray
Write-Host "  schtasks.exe <- drive.exe       => old build: UI autostart polling (CREATE_NO_WINDOW missing)" -ForegroundColor Gray
Write-Host "  drive-pwcmd.exe <- rclone.exe   => old drive-pwcmd: console subsystem" -ForegroundColor Gray
Write-Host "  cmd.exe <- rclone.exe           => rclone runs password-command through cmd" -ForegroundColor Gray
