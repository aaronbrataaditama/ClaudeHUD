# ClaudeHUD manual release checklist (spec §12)

See also `docs/manual-qa-pending.md` for additional deferred checks from Tasks 17-19 not covered here.

Run from `target\release\claudehud.exe` unless noted. Tick every line before a release.

## Light
- [ ] Two sessions open: strip green while one works, dim green when both are idle, hidden when both close.
- [ ] Permission prompt in a `--permission-mode default` session: yellow within ~1 s; green again after answering.
- [ ] `AskUserQuestion` and plan approval: yellow; clears on answer.
- [ ] 15-minute build in a session: stays green the whole time. (The most important regression check.)
- [ ] `CLAUDEHUD_FIXTURE=fixtures\snapshots\amber_quota.json`: amber. `red_quota_spent.json`: red. `off_no_sessions.json`: off.
- [ ] Kill a busy session's terminal: red; tooltip says "crashed mid-turn"; open and close the panel → no longer red.
- [ ] Restart ClaudeHUD with a stale busy registry file present: no red.

## Panel
- [ ] Hover the strip for ¼ s: the panel slides out from the strip. Sweeping across the strip opens nothing.
- [ ] Leave: closes ~⅓ s later; moving strip → panel keeps it open.
- [ ] Pin via the glyph and via a strip click; unpin; tray left-click toggles.
- [ ] Session name hover shows the working-folder tooltip, never clipped.
- [ ] Sub-agents appear under the session that spawned them; running → done.
- [ ] Mouse wheel scrolls the list when there are many sessions; header, usage and footer stay put.
- [ ] Plan label is right ("Team · Max 5x" here); Enterprise fixture shows "$50 of $600 spent".
- [ ] Clicking inside the panel never takes focus from the editor.
- [ ] Status link opens status.claude.com.

## Placement and system
- [ ] Edge Top/Left; Monitor switching across the mixed-DPI pair (strip is 132×4 logical px on both).
- [ ] Taskbar moved or resized: strip re-centres on the work area.
- [ ] Fullscreen app: strip hidden, returns afterwards.
- [ ] Win+L then unlock: nothing over the lock screen; strip back after unlock.
- [ ] Sleep and resume: correct state within 2 s.
- [ ] Explorer restart: the tray icon returns.
- [ ] Run on startup writes/removes `HKCU\…\Run\ClaudeHUD`; moving the exe re-points it on next launch.
- [ ] Second copy of the exe exits immediately.

## Portability and budgets
- [ ] Copy only `claudehud.exe` to a new folder on another Windows 11 machine: runs with no installer.
- [ ] Nothing is written under `%USERPROFILE%\.claude` (compare folder timestamps before and after a run).
- [ ] `scripts\budget.ps1`: exe < 2 MB, working set < 40 MB, CPU well under 1 s per minute.
- [ ] `cargo test --test smoke -- --ignored --test-threads=1` passes.
