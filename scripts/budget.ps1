# Release gate for §9: exe size and working set after 60 s.
# Hover the strip once during the 60 s wait so the panel's Direct2D resources are counted.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build --release
if ($LASTEXITCODE -ne 0) { exit 1 }
$exe = Resolve-Path "target\release\claudehud.exe"
$sizeKb = [math]::Round((Get-Item $exe).Length / 1KB)
Get-Process claudehud -ErrorAction SilentlyContinue | Stop-Process
$p = Start-Process $exe -PassThru
Write-Host "Running. Hover the strip once to open the panel. Measuring in 60 s..."
Start-Sleep 60
$p.Refresh()
$wsMb = [math]::Round($p.WorkingSet64 / 1MB, 1)
$cpu = [math]::Round($p.TotalProcessorTime.TotalSeconds, 2)
Stop-Process $p
"exe: $sizeKb KB (budget 2048 KB)"
"working set: $wsMb MB (budget 40 MB)"
"CPU used in 60 s: $cpu s (expect well under 1 s)"
if ($sizeKb -gt 2048 -or $wsMb -gt 40) { Write-Host "OVER BUDGET" -ForegroundColor Red; exit 1 }
Write-Host "Within budget" -ForegroundColor Green
