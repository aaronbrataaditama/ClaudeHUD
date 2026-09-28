# Manual QA pending (from Task 17)

These six checks from `docs/plans/claudehud/task-17-live-wiring.md` Step 4 were deliberately **not**
automated or delegated to a sub-agent, because each one touches something outside the build itself —
another live Claude Code session, the real network, or requires a human to judge "idle" or read a
tooltip. Do them with the user directly, whenever convenient (not blocking further implementation
tasks). Task 20's final `docs/manual-checklist.md` should fold these in.

Build and run first:
```powershell
Remove-Item Env:CLAUDEHUD_FIXTURE -ErrorAction SilentlyContinue
cargo build --release
Start-Process target\release\claudehud.exe
```

- [ ] **1. Tooltip matches `/usage`.** Hover the tray icon. Expected line one: `<this session's name>
  working` (or `… and N more working` if several sessions are busy). Line two, within ~5s of start:
  `N running · 5h X% · 7d Y%`, where the percentages match what `/usage` reports inside Claude Code.

- [ ] **2. Idle dims the strip.** Stop typing to Claude and wait until this session goes idle. Expected:
  the strip dims to green at 55% opacity rather than turning off or staying full-bright.

- [ ] **3. A permission prompt turns it yellow.** In another terminal, run
  `claude --permission-mode default` and ask it to run `dir`. When the permission prompt appears, the
  strip should turn yellow within ~1s; after answering, it returns to green (or dim, if now idle).

- [ ] **4. All sessions closed → off.** Close every Claude Code window. The strip should disappear
  entirely (`off`), and the tray creature should turn grey. Line two of the tooltip should still show
  usage percentages even with no sessions.

- [ ] **5. A crash latches red.** Kill a busy session's terminal window mid-turn (same technique as the
  Task 2 spike, step 7). Expected: the strip turns red, and the tooltip reads
  `… crashed mid-turn · click to acknowledge`. There's no panel to click yet (that's Task 18), so
  instead: Exit ClaudeHUD from the tray and restart it. After the restart, the stale registry file must
  **not** show red again (this is Review Focus item #2 in the plan — a startup-time stale file should
  never be treated as a fresh crash).

- [ ] **6. Network loss doesn't recolor anything.** Turn Wi-Fi off for ~10s. No colour should change;
  the tooltip's line two may show `usage unavailable · offline` at the next poll, once network is
  restored or the next scheduled poll fires.

Remember to exit `claudehud.exe` (tray → Exit, or `taskkill /F /IM claudehud.exe`) after each check that
starts it, so it doesn't hold the single-instance mutex for later runs.

## From Task 18 (hover panel)

- [ ] **7. Crash acknowledgement via the panel.** Kill a busy session's terminal window mid-turn (same
  technique as check 5 above) so the strip turns red. Hover the strip to open the panel — the crashed
  session's row should be visible while the panel is open, showing "Crashed mid-turn". Close the panel
  (move away and wait for the close timer, or click elsewhere). The strip should return to its normal
  (non-crashed) colour afterward — the crash acknowledges when the panel **closes**, not when it opens
  (a deliberate deviation from the spec's literal wording, recorded in `PLAN-CLAUDEHUD.md`/task notes:
  closing rather than opening means the crash row doesn't vanish while the user is still reading it).

