# Full local gate: formatting, lints, tests, release build, size budget.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Run($label, [scriptblock]$cmd) {
    Write-Host "== $label" -ForegroundColor Cyan
    & $cmd
    if ($LASTEXITCODE -ne 0) { Write-Host "FAILED: $label" -ForegroundColor Red; exit 1 }
}

Run "fmt"    { cargo fmt --check }
Run "clippy" { cargo clippy --all-targets -- -D warnings }
Run "test"   { cargo test }
Run "build"  { cargo build --release }

$exe = "target\release\claudehud.exe"
$size = (Get-Item $exe).Length
$budget = 2MB
if ($size -gt $budget) {
    Write-Host "FAILED: $exe is $size bytes, budget $budget" -ForegroundColor Red
    exit 1
}
Write-Host ("OK: {0} is {1:N0} KB (budget {2:N0} KB)" -f $exe, ($size / 1KB), ($budget / 1KB)) -ForegroundColor Green
