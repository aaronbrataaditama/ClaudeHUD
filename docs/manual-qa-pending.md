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

## From Task 18 verification: Windows 11 top-edge hit-testing

- [ ] **8. Investigate the maximized-window hit-test quirk together.** Confirmed during Task 18's
  review: when any window is maximized on the same monitor, Windows 11's Snap Layout hit-testing
  overlay (`TITLE_BAR_SCAFFOLDING_WINDOW_CLASS`) claims mouse input across the entire top edge of the
  screen, ahead of even `WS_EX_TOPMOST` windows — confirmed with `WindowFromPoint` returning that class
  instead of `ClaudeHUDStrip` at every point tested along the strip's span. The strip still draws on
  top and looks clickable, but a real mouse may not reach it while something is maximized. Worth
  checking together: (a) does this happen with *every* maximized window, or only ones with a custom
  Chromium/Electron-style title bar requesting full-width Snap Layout hover (the window that triggered
  it was titled "PLAN-SIDELIGHT desktop application", possibly Electron-based)? (b) does it also block
  genuine mouse hover (not just a simulated click), i.e. does `StripHover` ever fire while a window is
  maximized? (c) if it's a real, common-case problem, what's the right fix — a documented limitation,
  an inset/taller hit region, or something else? This should be resolved (or at least a decision made)
  before the final whole-branch review.

## From Task 19 (menu, settings, system events) — this task's own Step 3 is written as "ask the user"
for every item; none of it was automated by the implementer. Do these together:

- [ ] **9. First run + menu appearance.** Delete `claudehud.settings.json`, start the exe: a balloon
  "ClaudeHUD is running…" appears once, not on a second start. Right-click the tray icon: dark menu (if
  Windows is in dark mode) with "Show panel" bold-default, Edge▸, Monitor▸, Warn at▸, Run on startup,
  Open status page, Exit — in that order.
- [ ] **10. Edge switch.** Menu ▸ Edge ▸ Left moves the strip to the left edge, vertically centered;
  hovering slides the panel out to the right. Switch back to Top.
- [ ] **11. Monitor switch (needs a second display).** Menu ▸ Monitor ▸ the external display moves the
  strip there, correctly sized for that monitor's own DPI scale (not carrying over the laptop's ~164%
  sizing). Switch back to Primary.
- [ ] **12. Warn threshold.** Menu ▸ Warn at ▸ 80% while weekly usage is between 80-85% turns the strip
  amber; check the tooltip and panel meters recolor too.
- [ ] **13. Autostart round-trip.** Tick "Run on startup", confirm via
  `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v ClaudeHUD` that it points at the
  quoted exe path. Untick it, confirm the value is gone. This one genuinely writes the real Windows
  startup registry key — do it deliberately, not as an automated side effect.
- [ ] **14. Portable path fix.** Tick "Run on startup", Exit, copy the exe to a different folder, run it
  from there — the Run value should now point at the new path. Clean up (untick, delete the copy)
  afterward.
- [ ] **15. Fullscreen.** Start a fullscreen video or game — the strip disappears, and returns when
  fullscreen ends. **Do not delegate this to an agent** — needs real media/game and a human watching.
- [ ] **16. Lock/unlock.** Press Win+L, then unlock. The strip comes back; the panel never shows over
  the lock screen. **Never delegate this to an agent** — an agent that locks the screen has no way to
  unlock it and would strand the machine.
- [ ] **17. Sleep/wake.** Sleep the laptop and wake it — within 2s the strip reflects current state and
  the tooltip's usage refreshes. **Do not delegate this** — risks disconnecting the active session.
- [ ] **18. Taskbar/display change.** Move the taskbar to a different edge or change display scaling —
  the strip re-centers on the new work area. A real system-wide display change; do it deliberately.
- [ ] **19. Explorer restart.** Restart `explorer.exe` from Task Manager — the tray icon comes back
  afterward. This briefly restarts the entire Windows shell (taskbar, desktop) — disruptive enough that
  it should be a deliberate, watched action, not something an agent does on its own initiative.

