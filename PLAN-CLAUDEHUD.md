# ClaudeHUD — Claude Code status light — design spec (v3)

_Status: REVISED 2026-09-25. v1 (2026-09-17, `Downloads\PLAN-SIDELIGHT.md`) was approved; v2 removed
the WebView UI, the panel and all Claude Code hooks; v3 adds back a **native** hover panel (no
webview). UI mockup: `claudehud-mockup.html`. Next step: `git init` in
`C:\Projects\Personal\ClaudeHUD`, commit, run the week-one spike (§11 M2), then
`superpowers:writing-plans`._

## Context

Claude Code sessions run in terminals that sit behind the editor. The user wants an always-visible,
unobtrusive signal of whether Claude is running, whether it needs them, and how close the subscription
quota is to running out, plus details on demand (sessions, sub-agents, plan, spend, limits, service
status). No heavyweight app, and ClaudeHUD never modifies Claude Code's configuration.

Decisions:

| Topic | Decision |
|---|---|
| UI | A 4 px light strip; hovering it slides out a rounded detail panel |
| Stack | Rust, raw Win32 + Direct2D/DirectWrite via the `windows` crate, `serde_json`. No UI framework, no webview, no async runtime, no TLS crate |
| Portability | One exe, no installer, **never writes to `~/.claude`**. Only file written is its own settings |
| Waiting detection | Read Claude Code's own session registry (`status: "waiting"`). **No hooks, no statusline wrapper** |
| Edge | Top (default) or left, switchable from the panel or tray |
| Platform | Windows 11 first; platform code isolated in one module for later macOS/Linux |

**The governing rule (unchanged from v1):** `fold()` emits a colour only from positive, current
evidence. A collector that failed reports `unknown` and contributes no colour. No colour is ever derived
from elapsed time alone.

## 1. Light semantics

| Colour | Hex | Meaning | Triggers |
|---|---|---|---|
| Off | — (strip hidden) | No Claude session running | No live registry entry (and no latched red) |
| Green | `#4FBE86` | Claude session running | Any live session. Full brightness when `busy`/`shell`, 55% when all are `idle` |
| Yellow | `#E9DA4C` | Claude needs your input or confirmation | Any live session with `status: "waiting"` (§3.1) |
| Amber | `#F08A3E` | Quota nearly spent | Any usage limit or spend limit ≥ `warn_percent` (default **85**) and < 100 |
| Red | `#E0444E` | Limit reached, or something broke | Any usage/spend limit at 100; API error with retries exhausted or `error.rateLimits` set (§3.2); a session's process died while `busy` or `waiting`; status page reports "Claude Code" or "Claude API (api.anthropic.com)" not operational |

Priority: **red > yellow > amber > green > off.**

Colours were checked with the dataviz palette validator against the dark panel surface. Adjacent
pairs are distinguishable with normal vision (ΔE ≥ 15.6) and under colour-blind simulation (≥ 12.3),
and all four clear 3:1 contrast. The v2 yellow/amber pair (`#E8D44D`/`#E3A64C`) failed at ΔE 11.8 and
was replaced. Colour is never the only cue in the panel: every state also has a text label.

Rules:

- **Off wins over quota.** With no live session, quota amber/red and status-page red do not light the
  strip; the tray icon and panel still show them.
- **Crash red latches** and is the one exception to "off wins": a session that dies mid-turn keeps the
  strip red until the panel that shows it closes again (the acknowledgement), so the crash row stays
  readable while the panel is open.
- **The strip never animates.** Busy vs idle is a brightness step. The only motion anywhere is the panel
  sliding in and out, and that happens only when you hover.
- **No time-based colour.** A 14-minute turn is green, never red. `statusUpdatedAt` is a transition
  marker (measured frozen for 406 s on a working session), never a heartbeat.
- **Unknown is not a colour.** If the registry cannot be read, the light holds its previous state and
  the panel footer says why.
- A rate-limited poll of the usage endpoint is a collector error, **never** red.

## 2. Architecture

One process. The UI thread runs the Win32 message loop; one worker thread does the two HTTP polls and
hands results back with `PostMessage`.

```
src/
  main.rs            entry, message loop, 1 s WM_TIMER tick
  state.rs           Snapshot + fold(): the only place colour rules live
  collectors/        registry.rs transcript.rs subagents.rs usage.rs plan.rs status.rs
  hover.rs           pure hover state machine: step(state, Event) -> Vec<Action>
  panel/             layout.rs (pure: Snapshot -> list of positioned boxes/text runs)
                     render.rs (Direct2D/DirectWrite drawing of that list)
  platform/windows.rs  strip + panel windows, tray, WinHTTP, DPI, monitors, fullscreen detect,
                       power events, process creation time, autostart
  settings.rs        claudehud.settings.json
  icon.rs            pixel-creature map + runtime tray-icon rendering (§5.1)
assets/
  ClaudeHUD_icon.jpg source artwork (not shipped)
  claudehud.ico      multi-size app icon, generated once from the artwork and committed
build.rs             embeds claudehud.ico + the manifest as Win32 resources
```

Dependencies: `windows` (for the COM interfaces of Direct2D and DirectWrite), `serde`, `serde_json`;
build-only: `embed-resource` (compiles the `.rc` with the icon and manifest; adds nothing at runtime).
Direct2D, DirectWrite and WinHTTP ship with Windows, so nothing is bundled. WinHTTP honours the system
proxy (relevant on a corporate network) and uses the OS certificate store.

Release profile: `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`,
`strip = true`, static CRT. Font: Segoe UI Variable (the Windows 11 system font), so no fonts are
bundled.

### 2.1 The strip window

- 132 × 4 logical px (left edge: 4 × 132), centred on the chosen edge of the monitor's **work area**,
  2 px rounded ends.
- Extended styles `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST`, shown with
  `SW_SHOWNOACTIVATE`: never in the taskbar or Alt-Tab, never takes focus. Topmost re-asserted each
  tick.
- **Hit-testable, not click-through** (a change from v2), because it has to detect hover. The cost is
  that the 132 × 4 px strip itself no longer passes clicks to the window underneath; the rest of the
  edge is unaffected. Clicking the strip pins the panel open.
- Off = `SW_HIDE`. With no sessions there is nothing to hover, so the tray icon opens the panel instead.

### 2.2 The panel window

- `WS_POPUP` with the same extended styles, presented with `UpdateLayeredWindow` from a 32-bit
  premultiplied bitmap that Direct2D draws into. This gives anti-aliased **12 px rounded corners**, a
  soft drop shadow and per-pixel alpha with no DWM or webview dependency.
- Width 360 logical px. Height fits the content, capped at 80% of the work-area height; beyond that
  the session list scrolls with the mouse wheel while the header, usage block and status footer stay
  fixed.
- Created once at startup and hidden, so the reveal never waits on window creation.
- **Anchored to the strip**: top edge → the panel's top-centre sits 8 px below the strip and it slides
  **down**; left edge → the panel's left-middle sits 8 px right of the strip and it slides **right**.
  The panel is clamped inside the work area when the strip is near a corner.
- Redrawn only when the Snapshot changes, plus 1 Hz while visible so relative times stay current.
  Nothing is drawn while hidden.
- `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`, so clicking inside the panel (expanding a row, the pin,
  the status link) never steals focus from the editor.

### 2.3 Reveal, hide and pin (`hover.rs`)

- **Reveal**: `TrackMouseEvent(TME_HOVER)` on the strip with a 250 ms hover time, so the OS reports
  the dwell and nothing is polled. A pointer that is only passing across the edge moves on before
  250 ms and nothing opens.
- **Animation**: opens in 180 ms (translate 12 px → 0, opacity 0 → 1, ease-out) and closes in 140 ms.
  The 60 fps timer runs only during the animation.
- **Hide**: `TME_LEAVE` on both strip and panel; closes 300 ms after the pointer has left both, so
  crossing the 8 px gap or leaving briefly does not flicker.
- **Pin**: click the strip or the panel's pin glyph. A pinned panel stays until unpinned, the strip is
  clicked again, or the tray icon is clicked. Esc is not offered: the panel never has focus to receive it.
- Suppress the reveal on any edge that borders another monitor (from `EnumDisplayMonitors`), otherwise
  moving between displays would keep opening the panel.
- `step()` is pure and table-tested (§8).

### 2.4 DPI, monitors, limitations

Per-monitor DPI awareness v2 in the embedded manifest. Geometry in logical px via `GetDpiForMonitor`;
placement from `MONITORINFO.rcWork`; coordinates may be negative (this machine's secondary display has
origin x = −1920). Reposition on `WM_DPICHANGED`, `WM_DISPLAYCHANGE` and `WM_SETTINGCHANGE`; this
machine is mixed-DPI (~164% laptop beside a 100% external). Exclusive-fullscreen apps are detected with
`SHQueryUserNotificationState` and the strip is hidden. On resume (`WM_POWERBROADCAST`) and unlock
(`WM_WTSSESSION_CHANGE`) everything is refreshed and repositioned; while locked, the panel is suppressed.

**Known limitation, found during Task 18's manual verification and confirmed with a controlled test:**
when a window using Windows 11's newer immersive title bar (Snap Layout hover — Claude Desktop, VS
Code, Windows Terminal and similar Electron/WinUI3 apps) is maximized on the same monitor as the top
edge, the OS's own `TITLE_BAR_SCAFFOLDING_WINDOW_CLASS` hit-test overlay claims mouse input across the
entire top edge, ahead of even `WS_EX_TOPMOST` windows (verified with `WindowFromPoint` returning that
class instead of the strip's at every point along the strip's span). The strip still renders on top and
looks clickable, but real mouse hover/clicks may not reach it while such an app is maximized there. A
plain classic-chrome maximized window (tested with a bare WinForms window) does **not** cause this — the
scaffolding is specific to the newer immersive title bar, not a property of maximized windows in
general. No code-level fix is planned for v1: this is an OS shell behaviour outside what a topmost
window can override, and the tray icon remains a fully working fallback for status and access
regardless. Documented here rather than fixed; revisit if it proves disruptive in daily use.

## 3. Data sources

All reads. ClaudeHUD never writes to anything under `~/.claude`.

### 3.1 Session registry (`~/.claude/sessions/<pid>.json`) — green, yellow, crash red

Live sample (Claude Code 2.1.281):

```json
{"pid":22488,"sessionId":"34bea346-…","cwd":"C:\\Projects\\Personal\\ClaudeHUD",
 "startedAt":1790301420182,"procStart":"134347750190770188","version":"2.1.281",
 "kind":"interactive","entrypoint":"cli","pidDomain":"win32:laptop-6n3ets5a",
 "name":"claudehud-a4","status":"busy","statusUpdatedAt":1790302083994, …}
```

- **Status values**, from the Claude Code bundle: `busy | shell | idle | waiting`. `busy` and `shell`
  map to full green, `idle` to dim green, `waiting` to yellow. Unknown values count as `busy`.
- **The waiting signal.** Claude Code sets `status: "waiting"` plus a `waitingFor` string whenever it
  is blocked on the user. Strings seen in the bundle include `"approve the permission prompt"`,
  `"approve plan"`, `"input needed"`, `"dialog open"` and `"sandbox request"`. The panel shows
  `waitingFor` verbatim. The state is set and cleared by Claude Code itself, so no hooks and no
  anti-wedge rules are needed.
- **Liveness** = pid exists **and** its creation time (`GetProcessTimes`, FILETIME) equals `procStart`
  **and** `pidDomain` equals `win32:<hostname>`. Windows recycles pids.
- A dead entry is **dropped silently**, unless its last `status` was `busy` or `waiting`: that means it
  crashed mid-turn → latched red, and the panel shows it as a "crashed" row until acknowledged.
- Tolerate unknown fields; a missing `status` field is `unknown` for that session.
- **Security:** never read `sessions/*.key` (holds a `peerToken` capability secret); redact `*.key`
  paths in the log.
- **Fallback for older Claude Code** that never writes `waiting`: yellow when the transcript tail's last
  entry is a pending `tool_use` for `AskUserQuestion` or `ExitPlanMode`. No "pending > N s" rule.

### 3.2 Transcript tail (`~/.claude/projects/<slug>/<sessionId>.jsonl`)

Read only for live sessions, only when mtime/size changed, only the last 64 KB
(`seek(SeekFrom::End(-65536))`; transcripts reach 23 MB here). `slug` = cwd with `:` `\` `/` → `-`,
case preserved; fall back to a case-insensitive scan of `projects/*/<sessionId>.jsonl` (both `C--…`
and `c--…` exist on this machine). Drop a torn final line.

- **API error red** on `{"type":"system","subtype":"api_error"}` with `retryAttempt >= maxRetries` or
  non-null `error.rateLimits`; cleared by a later assistant line.
- **Model** from `message.model` on the last assistant line.
- **Context**: `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` of the last
  assistant line, shown as "last turn 132.6k in", an absolute figure with **no percentage**. The window
  size is not derivable (this machine runs a 1M window with no marker in `message.model`). After a
  `compact_boundary` in the tail, add "· compacted".
- **Pending sub-agent calls**: `tool_use` blocks named `Agent` or `Task` with no matching
  `tool_result`; their `input.description` and `input.subagent_type` label the sub-agent rows (§3.3).
- Tolerate other subtypes (`turn_duration`, `stop_hook_summary`, `away_summary`, `bridge_status`,
  `local_command`, `compact_boundary`).

### 3.3 Sub-agents (`~/.claude/projects/<slug>/<sessionId>/subagents/agent-*.jsonl`)

Scanned only while the panel is visible, or when a live session's transcript changes.

- Listed if the agent's transcript changed in the last 15 min. **Done** when its last assistant line has
  `stop_reason: "end_turn"`; **failed** on an exhausted `api_error`; otherwise **running** if written in
  the last 10 min, else **stopped**. (Background agents return a `tool_result` immediately, so the
  parent transcript cannot tell whether they are running.)
- Per-agent fields: type (`subagent_type`, e.g. `Explore`, `general-purpose`), description, elapsed
  time (first line timestamp → last line or now), model, tokens (sum of `usage` over its assistant
  lines), and tool-call count.
- Nested sub-agents (an agent spawning agents) are indented one level; deeper levels are collapsed
  into a count.

### 3.4 Plan and spend — the subscription label

- **Plan** from `~/.claude/.credentials.json` → `claudeAiOauth.subscriptionType` and `rateLimitTier`:
  `pro` → "Pro"; `max` + `…max_5x` → "Max 5x", `…max_20x` → "Max 20x"; `team` → "Team", with the
  Max tier appended when present ("Team · Max 5x", this machine: `team` + `default_claude_max_5x`);
  `enterprise` → "Enterprise". Anything else renders `rateLimitTier` verbatim.
- **Spend** from the usage response's `extra_usage` object. The Claude Code bundle's schema is
  `{is_enabled, monthly_limit, used_credits, utilization, currency}`, with amounts **in minor units of
  `currency`** (cents for USD). Rendered next to the plan: "Enterprise · $50 of $600 spent", or
  "$50 spent · no limit" when `monthly_limit` is null. Hidden when `extra_usage` is null. Non-USD
  currencies render with their ISO code ("€" only for EUR, otherwise "50 GBP").
- **Unverified here:** this machine is on Team, so the Enterprise shape of `extra_usage` has not been
  seen live. The parser treats every field as optional; the spike should check with an Enterprise
  account if one is available.

### 3.5 Usage limits (`GET https://api.anthropic.com/api/oauth/usage`) — amber, quota red

Headers `Authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20`, as in
`C:\Projects\Personal\AIUsage\Platform\ClaudeUsage.cs`.

- Re-read `~/.claude/.credentials.json` on every poll (Claude Code refreshes it). Never refresh, log or
  display the token.
- **Preferred shape: `limits[]`**, server-ordered rows `{kind, group, percent, resets_at,
  scope.model.display_name}`. Group rows under `group` in server order and classify on `kind`, never
  on a label. **Fallback shape**: named windows `five_hour`, `seven_day`, `seven_day_opus`,
  `seven_day_sonnet`, `seven_day_oauth_apps`, each `{utilization, resets_at}`. Render whichever is
  present; skip null windows.
- Labels: `five_hour` → "Current session · 5h", `seven_day` → "Weekly · all models", scoped weekly →
  "Weekly · {model}". Enterprise plans with no rate-limit windows show only the spend meter.
- Reset time as local time: "resets 14:32" if today, "resets Mon 09:00" if within 7 days, otherwise
  "resets 3 Oct".
- The spend limit (`extra_usage.utilization`) participates in amber/red exactly like a window.
- Poll every 300 s, immediately on resume, and once when the panel opens if the data is older than
  60 s. Rate-limited → backoff, "usage unavailable · rate limited", no colour. Missing/expired token →
  "Token expired — open Claude Code to refresh", no colour.

### 3.6 Service status (`GET https://status.claude.com/api/v2/summary.json`) — incident red

Hardcode `status.claude.com` (`status.anthropic.com` redirects). "Claude Code" and "Claude API
(api.anthropic.com)" drive red; "claude.ai" is shown but does not. Poll every 300 s with a descriptive
`User-Agent`. Shown in the panel footer (§4).

### 3.7 Polling

A single 1 s tick: `FindFirstFile` over `sessions\*.json` comparing mtimes, a liveness check per
entry, tail reads only where a transcript changed. A few dozen `stat` calls per second: negligible
CPU, no file-watcher crate, none of `ReadDirectoryChangesW`'s dropped-event problems.

## 4. Panel content

Mockup: `claudehud-mockup.html`. Dark surface (`#1C1D21`, 96% opacity) with a 1 px
`#FFFFFF14` border; Segoe UI Variable; text in three ink levels (primary `#ECECEE`, secondary
`#A4A6AD`, muted `#6E7078`). Status colours are used for dots and meter fills only, never for text.

Top to bottom:

1. **Header**: the app icon at 30 logical px (loaded from the embedded `.ico` with
   `LoadIconWithScaleDown` at 30 × DPI scale, drawn through WIC), plan label and spend ("Team · Max 5x",
   "Enterprise · $50 of $600 spent"), a one-line summary ("3 sessions · 1 needs you"), and the pin glyph
   on the right.
2. **Usage**: one row per limit: label, reset time, a meter, and the percentage. The meter fill carries
   severity (green < `warn_percent` ≤ amber < 100 = red) over a track that is a darker step of the
   same hue. Enterprise shows a spend meter ("$50 / $600 this month") in the same row format. This
   block never scrolls out of view.
3. **Sessions**, ordered crashed > waiting > busy > idle, then `statusUpdatedAt` descending. Each
   session row has:
   - a status dot, the session **name** (never wrapped), and a right-aligned short state: "Needs
     you" / "Working · 12m" / "Idle · 3m" / "Crashed mid-turn". "12m" is process uptime from
     `startedAt`.
   - a detail line: the `waitingFor` reason when waiting ("Approve the permission prompt"), then
     model · "last turn 132.6k in" · sub-agent count (and how many are running).
   - **hovering the name shows a tooltip with the full working folder** (`cwd`). It is drawn by
     ClaudeHUD inside the panel, not a native tooltip, and is clamped to the panel width with a
     middle ellipsis (`C:\Projects\…\ClaudeHUD`) so it can never be clipped.
   - **sub-agent rows** nested beneath, indented: a small status dot (running green / done muted /
     failed red), type, description (single line, ellipsised), elapsed, tokens.
   - Sessions with sub-agents are expanded by default when they are waiting or busy, collapsed when
     idle; click a session row to toggle.
   - Rendered list capped at 12 sessions, then a "+N idle" row.
4. **Footer — service status**: one line with a status dot and the overall description ("All systems
   operational" / "Partial outage · Claude API"), per-component dots for Claude Code, Claude API and
   claude.ai, "checked 2m ago", and a link that opens `https://status.claude.com` in the default
   browser. When a collector is `unknown` or `degraded`, a second muted line says which one ("usage
   unavailable · rate limited").

States with dedicated copy: **empty** ("No Claude sessions running" plus the usage block and footer),
**limit reached** (header "Weekly limit spent — new turns will fail until Mon 09:00"),
**token expired**, and **first run** (a one-time tray balloon: hover the light for details, right-click
the icon for settings).

## 5. Tray

- Icon: the pixel creature with a status badge (§5.1), generated at runtime; grey creature, no badge,
  when off.
- Tooltip (≤ 127 chars, two lines): line one is the reason for the colour, line two is counts and
  usage. Examples: "portal-service: approve the permission prompt" / "3 running · 5h 61% · 7d 88%";
  "Spend at 91% · $546 of $600 · resets 1 Oct" / "1 running"; "portal-service crashed mid-turn · click
  to acknowledge". If line one is too long, the session name is ellipsised first, never the reason.
  Mockup: `claudehud-mockup.html` §5.
- Left-click toggles the pinned panel (and acknowledges a latched red).
- Right-click: a native `TrackPopupMenu` menu, so it looks and behaves exactly like Windows'. Items:
  1. **Show panel** / **Hide panel**: the default item (bold, `SetMenuDefaultItem`), same as left-click
  2. separator
  3. Edge ▸ Top / Left (radio)
  4. Monitor ▸ Primary display / one item per monitor by friendly name (radio)
  5. Warn at ▸ 80% / 85% / 90% (radio); the current value is shown beside the item
  6. separator
  7. Run on startup (check)
  8. Open status page (opens `https://status.claude.com`)
  9. separator
  10. Exit
- Changes apply immediately and are saved to settings.
- Dark menu: call `SetPreferredAppMode(AllowDark)` (uxtheme ordinal 135) at startup so the menu follows
  the system dark theme. It is undocumented, so if the call is unavailable the menu is simply light.

### 5.1 Icon

Source artwork: `ClaudeHUD_icon.jpg` (2000 × 1091, the rounded tile spans ≈ x 590–1410, y 135–955): a
pixel creature orbited by green, yellow, amber and red balls, the same four states as the light.
Mockup: `claudehud-mockup.html` §6.

- **App icon (`claudehud.ico`)**, used for the exe in Explorer, the panel header and the first-run
  note. Frames: 256, 128, 64, 48 and 32 px use the full artwork, cropped to the tile and masked with
  its rounded-square alpha (radius ≈ 22% of the side) so the JPG's light background and shadow are
  dropped. 24, 20 and 16 px use a **simplified tile**: the same indigo → violet gradient
  (`#2F5BC4` → `#4A1F8C`) with only the pixel creature, because the orbits and balls turn to noise
  below 32 px. Generated once by a script and committed; 256 px stored PNG-compressed (≈ 60–100 KB).
- **Tray icon**, generated at runtime (`CreateIconIndirect`) at the size Windows asks for
  (`GetSystemMetrics(SM_CXSMICON)`: 16 px at 100%, 20 at 125%, 24 at 150%, 32 at 200%). It is the bare
  creature (no tile, so it sits on the taskbar like other tray icons) plus a **status badge**: a circle
  of radius 3.4/16 in the bottom-right, in the light's colour, with a 1 px cut-out ring so it separates
  from the body. Dim green = 55% badge; off = grey creature (`#8A8D94`), no badge. The badge carries the
  state, so the tray still reads at a glance.
- **Pixel map** (`icon.rs`), 12 × 7, `#` body `#DE7356`, `o` eye `#141414`, drawn on a 16-unit grid at
  (1, 3) and scaled by an integer factor with nearest-neighbour, so it is sharp at every DPI:
  ```
  ..########..
  ..#o####o#..
  ############
  ############
  ..########..
  ..#.#..#.#..
  ..#.#..#.#..
  ```
- **Source quality:** the artwork is a JPG, so edges carry compression noise at 256 px. A lossless PNG
  (or SVG) master would give a cleaner large icon; the JPG works if none exists.

## 6. Settings and persistence

`claudehud.settings.json` beside the exe when writable, else `%APPDATA%\ClaudeHUD\`. Fields: `edge`
(`top` | `left`), `monitor`, `warn_percent` (85), `usage_poll_s` (300), `status_poll_s` (300),
`autostart`, `first_run_done`. All editable from the tray menu.

Autostart uses `HKCU\…\CurrentVersion\Run\ClaudeHUD`, rewritten to the current exe path on each launch
if it has moved (portable), deleted when unticked. This is the only write outside the settings file.

## 7. Error handling

- Collectors fail independently: `healthy | degraded(reason) | unknown`; only `healthy` contributes
  colour; non-healthy collectors are named in the panel footer.
- Network limited to the two endpoints. No telemetry. Log beside settings, warnings only, 1 MB cap with
  one rollover, `*.key` paths redacted.
- Single instance via a named mutex.
- If Direct2D device creation fails (remote desktop, broken driver), fall back to the WARP software
  renderer; if that fails, the strip and tray still work and the panel is disabled with a tray notice.

## 8. Testing

1. **Golden-file `fold()` tests** — Snapshot JSON → expected colour and reason; one file per §1
   trigger, plus collisions (waiting + 92% → yellow; 100% + no sessions → off; latched crash + no
   sessions → red; spend 100% → red).
2. **Hover state machine** — `step()` table tests: pass-across under 250 ms (no reveal), dwell reveal,
   leave-and-return within 300 ms (stays open), strip → gap → panel traversal, pin/unpin, edge
   bordering another monitor (suppressed).
3. **Panel layout** — `layout(Snapshot)` is pure; assert row order, 12-row cap, tooltip clamping and
   ellipsis, scroll region, and that header/usage/footer stay fixed at the 80% height cap.
4. **Parser fixtures** — registry (each status, unknown fields, missing status, stale pid, recycled pid),
   transcript tails (`api_error` low/exhausted, pending `AskUserQuestion`, pending `Agent`, torn line,
   > 64 KB, `compact_boundary`), sub-agent files (running/done/failed, nested), usage JSON (`limits[]`,
   named windows, `extra_usage` in cents with null limit, Enterprise with no windows), plan labels
   (`pro`, `max` 5x/20x, `team` + Max tier, `enterprise`, unknown).
5. **Win32 smoke test** — extended styles of both windows; `GetForegroundWindow()` unchanged across
   reveal, click-inside and hide.
6. **CI budget checks** — exe < 2 MB; working set < 110 MB after 60 s with the panel shown once.

`CLAUDEHUD_FIXTURE=<Snapshot JSON>` replaces all collectors, for manual checks and golden inputs.

## 9. Targets

Exe under 2 MB. Working set under 110 MB, most of it Direct2D, DirectWrite and the GPU driver stack
they pull in (v1 budgeted 150 MB for WebView2). **Revised from an original 40 MB target** (2026-09-28,
Task 20). Measured directly on real hardware, sampled repeatedly to rule out a leak:
- ~54 MB baseline with the panel never opened — confirmed via loaded-module inspection to be dominated
  by the Intel graphics driver's shader compiler and user-mode driver DLLs (`igc64.dll`,
  `igd10umt64xe.dll`) that any hardware-accelerated Direct2D app pulls in on this GPU, not ClaudeHUD's
  own allocations.
- ~95 MB stable steady-state once the panel has been opened once (first real text layout/font-shaping
  and icon-bitmap costs) — confirmed flat across 60+ seconds and across 6 repeated open/close cycles,
  so this is a one-time cost, not a leak. The `Renderer`/`Surface` reuse logic was reviewed and is
  correct: one `Renderer` for the process lifetime, one `Surface` reused unless its pixel dimensions
  change.
110 MB leaves real headroom above the measured ~95 MB peak while staying well below the 150 MB baseline
it replaced. Idle CPU effectively zero: one 1 s timer, two HTTP calls every 5 minutes, no drawing while
hidden.

## 10. Out of scope for v1

Claude Code hooks of any kind; global hotkey; controlling or pausing sessions; token refresh by
ClaudeHUD; macOS/Linux; themes (dark only); multiple strips; org or workspace names.

## 11. Milestones

1. Cargo scaffold, release profile, `claudehud.ico` generated and embedded via `build.rs`, strip
   window with a static colour and window flags, top/left placement across the mixed-DPI pair, tray
   with the creature icon and Exit, CI size check.
2. **Spike (1–2 h), decision gate.** With a background logger polling `sessions\*.json`, trigger a
   permission prompt (in `default` mode, since auto mode rarely prompts), `AskUserQuestion`, plan
   approval and an MCP elicitation. Confirm each writes `status: "waiting"` promptly and clears on
   answer; record the `waitingFor` strings. Dump one live usage response to confirm `limits[]` vs
   named windows and `extra_usage`. Anything missing falls back as described in §3.
3. Registry collector with liveness, `fold()` for off/green/yellow and crash latch, golden tests, live
   strip.
4. Usage, plan and status collectors over WinHTTP; amber/red; tray tooltip and menu.
5. Panel window: Direct2D bitmap + `UpdateLayeredWindow`, hover state machine, slide animation, pin,
   layout of header/usage/sessions/footer, name tooltip, empty/limit/expired/first-run states.
6. Transcript tail and sub-agents: model, context figure, sub-agent rows, API-error red.
7. Settings + autostart, fullscreen hide, power/unlock refresh, single instance, WARP fallback, manual
   checklist.

## 12. Verification

`cargo run --release`. Open two sessions: strip green, dim-green when both idle, hidden when both
close. Hover the strip: the panel slides out within ~430 ms (250 ms dwell + 180 ms animation) and
closes 300 ms after the pointer leaves. Sweep the pointer quickly across the strip: nothing opens.
Hover a session name: the tooltip shows the full folder. Start a task that spawns sub-agents: rows
appear nested, running → done. Trigger a permission prompt: yellow within ~1 s, green on answer.
Run a 15-minute build: stays green throughout (the most important regression check).
`CLAUDEHUD_FIXTURE` at 86% → amber, 100% → red, 100% with no sessions → off, Enterprise fixture →
"$50 of $600 spent" and spend meter. Kill a session mid-turn: red until the panel is opened. Click
inside the panel: editor keeps focus. Switch edge and monitor across the mixed-DPI pair. Sleep/resume.
Check exe size and working set.
