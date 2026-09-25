# Saves one live response from the usage endpoint to fixtures/usage/.
# The token is read into memory only; it is never printed or written.
$ErrorActionPreference = "Stop"
$credPath = Join-Path $env:USERPROFILE ".claude\.credentials.json"
$o = (Get-Content -Raw -LiteralPath $credPath | ConvertFrom-Json).claudeAiOauth
"subscriptionType=$($o.subscriptionType)  rateLimitTier=$($o.rateLimitTier)"
$headers = @{ Authorization = "Bearer $($o.accessToken)"; "anthropic-beta" = "oauth-2025-04-20" }
$r = Invoke-WebRequest -Uri "https://api.anthropic.com/api/oauth/usage" -Headers $headers -UseBasicParsing
$outDir = Join-Path $PSScriptRoot "..\fixtures\usage"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$out = Join-Path $outDir ("live-{0}.json" -f (Get-Date -Format yyyyMMdd))
[IO.File]::WriteAllText($out, $r.Content)   # UTF-8 without BOM
"HTTP $($r.StatusCode) -> $out"
"top-level keys: " + ((($r.Content | ConvertFrom-Json).PSObject.Properties.Name) -join ", ")
