# Task 2: Spike: verify the waiting signal and the usage response (decision gate)

**This task needs the human.** The agent writes the two scripts, then asks the user to run them and to trigger prompts in a separate Claude Code session. The agent must not read `~/.claude/.credentials.json` itself (project settings deny it); the user runs the usage script with `! powershell -File scripts/dump-usage.ps1` so the token never enters the agent's context.

**Goal:** Confirm before building on it that (a) Claude Code writes `status: "waiting"` + `waitingFor` to `~/.claude/sessions/<pid>.json` for permission prompts, `AskUserQuestion`, plan approval and (optionally) MCP elicitation, and clears it on answer; (b) `procStart` equals the process creation FILETIME; (c) the live usage response shape (`limits[]` vs named windows, `extra_usage`). Record results in `docs/spike-results.md` and save a usage fixture.

**Files:**
- Create: `scripts/watch-registry.ps1`, `scripts/dump-usage.ps1`
- Create: `docs/spike-results.md`
- Create (by the user's run): `fixtures/usage/live-YYYYMMDD.json`

**Interfaces:**
- Consumes: nothing from the crate.
- Produces: `docs/spike-results.md` (read by Tasks 4, 7 and 13 if it records deviations) and a real usage fixture for Task 7.

---

- [ ] **Step 1: Registry watcher**

`scripts/watch-registry.ps1`:

```powershell
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
```

- [ ] **Step 2: Usage dump (run by the user, never by the agent)**

`scripts/dump-usage.ps1`:

```powershell
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
```

- [ ] **Step 3: Ask the user to run the checks**

Send the user exactly this checklist:

1. In terminal A: `powershell -ExecutionPolicy Bypass -File scripts/watch-registry.ps1`
2. In terminal B, start a fresh session in **default** permission mode (auto mode rarely prompts): `claude --permission-mode default`
3. In B, ask: `run the shell command "dir" in this folder` → a permission prompt appears. Wait 3 s, then answer it.
4. In B, ask: `use the AskUserQuestion tool to ask me a multiple-choice question` → answer it.
5. In B, press Shift+Tab until plan mode, ask `plan a one-line README`, and when the plan-approval prompt appears wait 3 s, then approve.
6. Optional, only if an MCP server that asks for input is configured: trigger an elicitation.
7. Exit B normally (`/exit`). Start B again and, while Claude is working on a long request, kill its terminal window (close it). Note what A shows for that session afterwards (the file should stay with status `busy`).
8. Run `! powershell -ExecutionPolicy Bypass -File scripts/dump-usage.ps1` in this Claude session (the `!` prefix runs it as the user).
9. Paste terminal A's output into the chat.

- [ ] **Step 4: Record results**

Create `docs/spike-results.md` from what the user pasted, using this template:

```markdown
# Spike results (Task 2), YYYY-MM-DD, Claude Code <version from registry>

| Trigger | status seen | waitingFor | latency after prompt appeared | cleared on answer? |
|---|---|---|---|---|
| Permission prompt (Bash) | | | | |
| AskUserQuestion | | | | |
| Plan approval | | | | |
| MCP elicitation | | | | |

- procStart matches process creation FILETIME: yes / no (paste one line)
- Killed mid-turn: registry file kept? status left as:
- Usage response top-level keys:
- `limits[]` present: yes / no. Named windows present:
- `extra_usage` present: yes / no. Shape:
- Plan: subscriptionType = , rateLimitTier =

## Decision
```

- [ ] **Step 5: Apply the decision gate**

- **All three required triggers (permission, AskUserQuestion, plan approval) show `status=waiting` within ~1 s and clear on answer** → write "Proceed as specified." under Decision.
- **Permission prompt does not produce `waiting`** → stop. Report to the user: the transcript fallback (§3.1) covers only `AskUserQuestion`/`ExitPlanMode`, so permission prompts would be undetectable without hooks. Ask how to proceed. Do not continue to Task 3 until they answer.
- **`procStart` does not equal the actual FILETIME but is within 1 s** → fine; Task 4 already uses a 1 s tolerance. Note it. **If it differs by more than 1 s**, note the typical difference; Task 4's `PROC_START_TOLERANCE` must be raised to cover it (tell the Task 4 implementer).
- **Usage has neither `limits[]` nor any of `five_hour`/`seven_day`** → report the keys to the user before Task 7.

- [ ] **Step 6: Commit**

Open the saved `fixtures/usage/live-*.json` and check it holds no personal identifiers (email, org name, account UUID). If it does, replace those values with `"redacted"` before committing.

```powershell
git add scripts/watch-registry.ps1 scripts/dump-usage.ps1 docs/spike-results.md fixtures/usage
git commit -m "docs: spike results for waiting signal and usage shape"
```
