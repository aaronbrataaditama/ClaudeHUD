# ClaudeHUD Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Each task lives in its own file in this folder; an implementer needs **this README + their task file + the spec sections the task cites**, nothing else.

**Goal:** Build ClaudeHUD, a portable single-exe Windows tray app that shows Claude Code's state as a 4 px light strip on the screen edge, with a native hover panel for sessions, sub-agents, plan, spend, usage limits and service status.

**Architecture:** One Rust crate with a platform-free library (`src/`: data model, parsers, `fold()`, hover state machine, panel layout, tooltip, icon pixels, collector, fetch scheduling) that is fully unit-tested on any machine, plus a Windows-only `src/platform/` layer (Win32 windows, tray, Direct2D/DirectWrite rendering, WinHTTP, process probing) that turns that data into pixels. One UI thread runs the message loop and a 1 s tick; one worker thread does the two HTTP polls.

**Tech Stack:** Rust stable ≥ 1.82 (edition 2021, `x86_64-pc-windows-msvc`), `windows` 0.61 (Win32 + Direct2D + DirectWrite + WIC + WinHTTP), `serde` 1 + `serde_json` 1, build-only `embed-resource` 3. PowerShell 5.1+ for helper scripts.

**Spec:** `PLAN-CLAUDEHUD.md` (repo root). Section numbers like §3.1 refer to it. UI reference: `claudehud-mockup.html` (open in a browser; it is a picture, not code to port).

## Global Constraints

Every task implicitly includes these. Values copied from the spec.

- Runtime dependencies are exactly `windows`, `serde`, `serde_json`; build-only `embed-resource`. **No other crates, including dev-dependencies.** No async runtime, HTTP crate, GUI crate, image crate, time crate, regex, logging crate, tempfile.
- **Never write, create or delete anything under `~/.claude`.** Never open `~/.claude/sessions/*.key`. Never log/print/display the OAuth access token (use the `Secret` type from Task 7). Tests use fixtures under `fixtures/` or temp dirs, never the real `~/.claude`.
- No Claude Code hooks, no statusline wrapper, no edits to `~/.claude/settings.json`.
- Colours: green `#4FBE86`, yellow `#E9DA4C`, amber `#F08A3E`, red `#E0444E`, off `#3B3F46`; idle green at 55% (`DIM_ALPHA = 0.55`). Priority red > yellow > amber > green > off.
- `fold()` (Task 9, `src/state.rs`) is the only place colour rules live. Absence of evidence is not a colour; non-`Healthy` collectors contribute no colour; no colour from elapsed time alone.
- Default `warn_percent` = 85; allowed 80/85/90 from the menu (settings clamp 50..=99). Usage and status polls every 300 s.
- Strip 132 × 4 logical px (left edge 4 × 132), centred on the monitor **work area** edge. Panel 360 logical px wide, 12 px corner radius, height ≤ 80% of work-area height, 8 px gap from the strip, slides 12 px, open 180 ms ease-out, close 140 ms. Reveal after 250 ms hover; hide 300 ms after pointer leaves strip and panel.
- Budgets: release exe < 2 MB; working set < 40 MB after 60 s idle with the panel shown once.
- Tooltip ≤ 127 UTF-16 units, two lines.
- Files read from Claude Code are undocumented formats: every field optional, unknown fields ignored, strip a UTF-8 BOM before parsing, a parse failure degrades that collector instead of panicking. No `unwrap()`/`expect()` in non-test code except in `main`/startup where failure means the app cannot run at all.
- Geometry in logical px everywhere except `src/platform/` and `src/geometry.rs` (which converts using the monitor scale).
- Endpoints (the only network access): `https://api.anthropic.com/api/oauth/usage` with headers `Authorization: Bearer <token>` and `anthropic-beta: oauth-2025-04-20`; `https://status.claude.com/api/v2/summary.json`. User-Agent `ClaudeHUD/0.1 (+https://github.com/; Claude Code status light)`.
- Commit after every task with Conventional-Commit style messages (`feat(registry): …`). Run `cargo test` and `cargo clippy --all-targets -- -D warnings` before committing.

## Review Focus

Inputs the spec implies but no happy-path test exercises. Each has a pinned test in the task named.

1. **Registry file caught mid-rewrite** (torn JSON for one tick) must not drop the session and flash the strip off/on. Expected: the last good parse of that file is reused for up to 5 ticks. → Task 13 `torn_registry_file_reuses_last_good_entry`.
2. **Stale `busy` registry files left from before ClaudeHUD started** (e.g. after a reboot) must not light red at launch. Expected: crash red only for a session ClaudeHUD saw alive during this run. → Task 9 `dead_busy_entry_never_seen_alive_does_not_latch`.
3. **UTF-8 BOM and CRLF** in JSON/JSONL files (PowerShell-written fixtures, Windows editors). Expected: parsed normally. → Task 4 `parses_entry_with_bom`, Task 5 `crlf_lines_parse`.
4. **Token expiry/401 while Claude Code has not refreshed yet.** Expected: "Token expired — open Claude Code to refresh", no colour, last usage values kept (stale) in the panel, no retry storm. → Task 13 `http_401_maps_to_token_expired_and_keeps_stale_value`.
5. **Multiple usage windows at 100% at once.** Expected: red names the one that resets last (the one that actually blocks you). → Task 9 golden `red_two_windows_spent.json`.

---

## File map

```
Cargo.toml, build.rs, rust-toolchain.toml, .cargo/config.toml
assets/claudehud.rc, assets/claudehud.manifest, assets/claudehud.ico, assets/ClaudeHUD_icon.jpg
scripts/check.ps1            fmt + clippy + test + release build + size budget
scripts/watch-registry.ps1   spike: log registry status transitions
scripts/dump-usage.ps1       spike: save one live usage response (run by the human)
scripts/make-icon.ps1        builds assets/claudehud.ico from the artwork
src/main.rs                  entry; calls platform::app::run()
src/lib.rs                   module list
src/model.rs                 ALL shared data types (Task 3)
src/timefmt.rs               ISO-8601 parse, civil dates, LocalTime, now_ms (Task 3)
src/format.rs                display strings: tokens, durations, money, resets, ellipsis (Task 3)
src/collectors/mod.rs        strip_bom + module list
src/collectors/tail.rs       read_tail / read_head / complete_lines (Task 5)
src/collectors/registry.rs   sessions/*.json → RegistryScan, liveness (Task 4)
src/collectors/transcript.rs transcript tail → TranscriptFacts (Task 5)
src/collectors/subagents.rs  subagents/agent-*.{jsonl,meta.json} → Vec<Subagent> (Task 6)
src/collectors/credentials.rs .credentials.json → Credentials, plan_label, Secret (Task 7)
src/collectors/usage.rs      usage JSON → Usage (Task 7)
src/collectors/status.rs     summary.json → ServiceStatus (Task 8)
src/state.rs                 fold(), ordered_sessions() (Task 9)
src/latch.rs                 CrashLatch (Task 9)
src/fixture.rs               CLAUDEHUD_FIXTURE loader (Task 9)
src/tooltip.rs               tray tooltip text (Task 10)
src/icon.rs                  creature pixel map, tray icon + strip pixels (Task 10)
src/settings.rs              Settings, Edge, load/save/path (Task 11)
src/geometry.rs              Rect, MonitorInfo, strip/panel placement (Task 11)
src/hover.rs                 hover/pin state machine (Task 11)
src/panel/mod.rs, src/panel/layout.rs   pure panel layout → draw ops + hit regions (Task 12)
src/collect.rs               Collector (tick/apply/snapshot), HttpGet, fetch_usage, fetch_status, Schedule (Task 13)
src/platform/*.rs            Windows only (Tasks 15–19)
tests/common/mod.rs          TempDir helper for integration tests
tests/golden.rs              runs fixtures/snapshots/*.json through fold()
tests/collect.rs             Collector against a temp claude dir
tests/smoke.rs               #[ignore] Win32 smoke test (Task 20)
fixtures/                    snapshots/, registry/, transcripts/, usage/, status/
```

## Tasks

| # | File | Deliverable | Needs |
|---|---|---|---|
| 1 | [task-01-toolchain-scaffold.md](task-01-toolchain-scaffold.md) | Rust installed; crate builds; `scripts/check.ps1` passes | — |
| 2 | [task-02-spike.md](task-02-spike.md) | **Human-in-the-loop.** Waiting signal + usage shape verified; `docs/spike-results.md` | 1 |
| 3 | [task-03-model-time-format.md](task-03-model-time-format.md) | `model.rs`, `timefmt.rs`, `format.rs` + tests | 1 |
| 4 | [task-04-registry.md](task-04-registry.md) | Registry parser + liveness + dir scan | 3 |
| 5 | [task-05-transcript.md](task-05-transcript.md) | Tail reader + transcript facts | 3 |
| 6 | [task-06-subagents.md](task-06-subagents.md) | Sub-agent listing | 5 |
| 7 | [task-07-credentials-usage.md](task-07-credentials-usage.md) | Credentials, plan label, usage parser | 3 |
| 8 | [task-08-status.md](task-08-status.md) | Status page parser | 3 |
| 9 | [task-09-fold-latch-golden.md](task-09-fold-latch-golden.md) | `fold()`, ordering, crash latch, fixture loader, golden tests | 4–8 |
| 10 | [task-10-tooltip-icon.md](task-10-tooltip-icon.md) | Tooltip text; tray-icon and strip pixels | 9 |
| 11 | [task-11-settings-geometry-hover.md](task-11-settings-geometry-hover.md) | Settings, placement maths, hover state machine | 3 |
| 12 | [task-12-panel-layout.md](task-12-panel-layout.md) | Pure panel layout | 9, 11 |
| 13 | [task-13-collector-fetch.md](task-13-collector-fetch.md) | Collector, HTTP fetch logic, poll schedule | 4–9 |
| 14 | [task-14-app-icon.md](task-14-app-icon.md) | `assets/claudehud.ico` embedded in the exe | 1 |
| 15 | [task-15-platform-services.md](task-15-platform-services.md) | Process probe, local time, WinHTTP, system helpers | 13 |
| 16 | [task-16-strip-tray-app.md](task-16-strip-tray-app.md) | Visible strip + tray from a fixture | 10, 11, 15 |
| 17 | [task-17-live-wiring.md](task-17-live-wiring.md) | Live data → strip + tray | 16 |
| 18 | [task-18-panel-window.md](task-18-panel-window.md) | Hover panel rendered with Direct2D | 12, 17 |
| 19 | [task-19-system-integration.md](task-19-system-integration.md) | Menu actions, DPI/monitor/power/lock/fullscreen, autostart, first run | 18 |
| 20 | [task-20-smoke-verify.md](task-20-smoke-verify.md) | Smoke test, budgets, manual checklist | 19 |

Tasks 3–13 are pure Rust and testable anywhere; do them strictly in order (later tasks import earlier types). Tasks 15–19 are Windows-only and are verified by running the app.

## Interface index (names later tasks rely on)

Defined in Task 3 (`src/model.rs`): `SessionStatus`, `TranscriptFacts`, `AgentState`, `Subagent`, `Session`, `CrashedSession`, `Health`, `Collected<T>`, `Limit`, `Spend`, `Usage`, `ComponentState`, `Component`, `ServiceStatus`, `Snapshot`, `Colour`, `Reason`, `Light`, `DIM_ALPHA`, `NOT_COLLECTED`.
Task 3 (`src/timefmt.rs`): `LocalTime`, `parse_iso8601`, `utc_parts`, `days_from_civil`, `civil_from_days`, `now_ms`.
Task 3 (`src/format.rs`): `tokens`, `uptime`, `elapsed`, `ago`, `money`, `reset_label`, `spend_reset_label`, `pct_label`, `middle_ellipsis`, `sentence_case`, `truncate_chars`, `limit_noun`, `short_limit`, `short_component`, `component_state_text`, `model_name`, `folder_name`.
Task 4: `collectors::registry::{RegistryEntry, RegistryScan, ProcessProbe, Liveness, parse_entry, liveness, scan_dir}`; `collectors::strip_bom`.
Task 5: `collectors::tail::{TAIL_BYTES, read_tail, read_head, complete_lines}`; `collectors::transcript::{slug_for_cwd, find_transcript, parse_tail}`.
Task 6: `collectors::subagents::{AgentMeta, AgentTail, parse_meta, parse_agent_tail, first_timestamp_ms, classify, list_subagents}`.
Task 7: `collectors::credentials::{Secret, Credentials, parse_credentials, plan_label}`; `collectors::usage::parse_usage`.
Task 8: `collectors::status::{parse_status, WATCHED, RED_COMPONENTS}`.
Task 9: `state::{fold, ordered_sessions, severity_colour}`; `latch::CrashLatch`; `fixture::{parse_snapshot, load_snapshot, anchor}`.
Task 10: `tooltip::tooltip`; `icon::{CREATURE, Badge, render_tray_icon, render_strip}`.
Task 11: `settings::{Settings, Edge, load, save, settings_path}`; `geometry::{Rect, MonitorInfo, strip_rect, panel_rect, max_content_h, pick_monitor, edge_borders_other_monitor, to_px}`; `hover::{Hover, Event, Action}`.
Task 12: `panel::layout::{layout, is_expanded, Ctx, Layout, Op, TextOp, Font, Ink, Align, RectF, Hit, HitRegion, ViewState, Measure, W, RADIUS}`.
Task 15: `log::{init, warn, append_to, redact}`; `platform::{process::WinProbe, localtime::local_parts, http::WinHttp, system::*}`.
Task 16–19: `platform::{win, layered, monitors, tray, worker, render, menu, app::run}`.
Task 13: `collect::{Collector, HttpGet, HttpResponse, UsageError, UsageOutcome, fetch_usage, fetch_status, Schedule}`.

## Notes for implementers

- **Win32 code (Tasks 15–19) is a reference implementation written against `windows` 0.61.** The crate's exact signatures drift between versions (e.g. `Option<HWND>` vs `HWND`, `Result<()>` vs `BOOL`, where `BOOL` lives). If the compiler disagrees with a call, fix the *signature-level* detail using the compiler message or docs.rs for the pinned version; do not change behaviour. Keep the version pinned in `Cargo.toml`.
- Windows `wndproc` callbacks re-enter (e.g. `ShowWindow` sends messages synchronously). All app state is in a `thread_local! RefCell`; wndprocs must use `try_borrow_mut()` and fall back to `DefWindowProcW` when it is already borrowed. Never call `borrow_mut()` in a wndproc. The release profile uses `panic = "abort"`, so a double borrow would kill the app.
- Run commands from the repo root in PowerShell unless a step says otherwise.
