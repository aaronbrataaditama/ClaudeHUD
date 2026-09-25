# ClaudeHUD

A tiny Windows tray app: a 4 px light strip on the screen edge that shows Claude Code's state
(off / green / yellow / amber / red), with a native hover panel for sessions, sub-agents, plan, spend,
usage limits and service status.

- **Design spec (source of truth):** `PLAN-CLAUDEHUD.md`. Section numbers (§) below refer to it.
- **UI mockup:** `claudehud-mockup.html` (open in a browser). Visual reference only; never port it.
- **Spike results:** `docs/spike-results.md` (after Task 2): what the live registry and usage API actually do.
- **Implementation plan:** `docs/plans/claudehud/README.md` (index, global constraints, review focus)
  plus one `task-NN-*.md` file per task. Work task by task, in order, and tick the checkboxes as you go.
- **Progress tracker:** `.claude/PROGRESS.md`. **Read it first when resuming.** Update it whenever a
  task starts, finishes, gets a review verdict, hits a blocker or changes a decision, and commit it with
  the task. The user may pause at any point, so it must always reflect the true state.

## How implementation runs

- Subagent-driven: the main session coordinates and delegates every task to sub-agents, then updates
  `.claude/PROGRESS.md`.
- Use the cheapest model that fits: **haiku** for verbatim-code and mechanical tasks, **sonnet** for
  logic, Win32/Direct2D work and reviews, **opus** only for the final review or after two sonnet
  failures. The per-task assignment and escalation rule are in `.claude/PROGRESS.md`.

## Commands

```
cargo build --release          # portable exe: target\release\claudehud.exe
cargo test                     # unit, golden and fixture tests (+ platform tests on Windows)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
$env:CLAUDEHUD_FIXTURE="fixtures\snapshots\yellow_waiting_beats_amber.json"; cargo run --release   # fake data
```

Toolchain: stable Rust, `x86_64-pc-windows-msvc`, MSVC Build Tools + Windows SDK 10.0.26100.

## Hard rules

- **Read-only on Claude Code.** Never write, create or delete anything under `~/.claude`. Never read
  `~/.claude/sessions/*.key` or log a path matching `*.key`. Never log, print or display the OAuth token.
  In tests, use fixtures under `fixtures/`, never the real `~/.claude`.
- **No hooks, no statusline wrapper, no edits to `~/.claude/settings.json`.** Waiting detection comes
  from the session registry (§3.1).
- **Lightweight.** Runtime dependencies are exactly `windows`, `serde`, `serde_json`; build-only
  `embed-resource`. Do not add a crate (HTTP, async, GUI, image, logging, time, regex) without asking.
  HTTP is WinHTTP, drawing is Direct2D/DirectWrite, time maths is hand-written.
- **`fold()` in `src/state.rs` is the only place colour rules live.** Collectors report facts, never
  colours. Absence of evidence is not a colour; `unknown`/`degraded` collectors contribute nothing.
  No colour is ever derived from elapsed time alone.
- **Keep logic pure and testable.** Parsing, `fold()`, the hover state machine, panel layout, formatting
  and the icon pixel map take plain data and return plain data. Only `src/platform/` calls Win32. Tests
  must pass without a desktop session.
- Status colours are fixed (§1): green `#4FBE86`, yellow `#E9DA4C`, amber `#F08A3E`, red `#E0444E`,
  off `#3B3F46`. Don't retune them without re-running the palette check.

## Conventions

- Geometry in logical px; convert with the monitor's DPI only in `src/platform/`.
- Times: Registry values are Unix ms; `procStart` is a Windows FILETIME string; API times are ISO 8601.
- Undocumented formats: every field optional, unknown fields ignored, a parse failure marks the
  collector `unknown` rather than panicking. No `unwrap()`/`expect()` outside tests and `main` startup.
- Commit per task with a message like `feat(registry): liveness via procStart`; run `cargo test` and
  `cargo clippy` first.
