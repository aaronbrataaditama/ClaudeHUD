# Logs every status / waitingFor transition in ~/.claude/sessions/*.json.
# Read-only: never opens *.key files, never writes anything.
$dir = Join-Path $env:USERPROFILE ".claude\sessions"
$last = @{}
Write-Host "Watching $dir  (Ctrl+C to stop)"
while ($true) {
    Get-ChildItem -Path $dir -Filter *.json -ErrorAction SilentlyContinue | ForEach-Object {
        try { $j = Get-Content -Raw -LiteralPath $_.FullName | ConvertFrom-Json } catch { return }
        $sig = "$($j.status)|$($j.waitingFor)"
        if ($last[$_.Name] -ne $sig) {
            $last[$_.Name] = $sig
            $p = Get-Process -Id $j.pid -ErrorAction SilentlyContinue
            $actual = if ($p) { $p.StartTime.ToFileTimeUtc() } else { "dead" }
            $match = if ($p -and ("$($j.procStart)" -eq "$actual")) { "procStart=OK" } else { "procStart=$($j.procStart) actual=$actual" }
            "{0:HH:mm:ss.fff}  {1,-22} status={2,-8} waitingFor='{3}'  {4}" -f (Get-Date), $j.name, $j.status, $j.waitingFor, $match
        }
    }
    Start-Sleep -Milliseconds 250
}
