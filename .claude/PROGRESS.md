# ClaudeHUD implementation progress

Single source of truth for where the implementation stands. **Update this file after every task
step that changes state** (task started, task done, review verdict, blocker, decision), and commit it
with the task's commit. Anyone resuming, human or model, reads this file first.

- Plan: `docs/plans/claudehud/README.md` + `docs/plans/claudehud/task-NN-*.md`
- Spec: `PLAN-CLAUDEHUD.md` · Rules: `CLAUDE.md` · Mockup: `claudehud-mockup.html`
- Execution method: **subagent-driven** (`superpowers:subagent-driven-development`)

## Resume here

- **Current task:** Task 20 done — **all 20 implementation tasks are complete**. Nothing left before
  the final whole-branch review (opus).
- **Next action:** waiting on user go-ahead to start the final whole-branch review. Separately,
  whenever convenient: work through `docs/manual-qa-pending.md`'s checklist together (items 1-7 from
  Tasks 17-18, items 9-19 from Task 19; item 8 already resolved) and `docs/manual-checklist.md` (the
  spec §12 release checklist, entirely unticked — needs a human to walk through it).
- **Branch:** `main` at `9be284f`.
- **Waiting on user:** confirmation to proceed to the final whole-branch review
- **IMPORTANT — git history was rewritten on 2026-09-28 (see decisions log).** Every commit hash
  mentioned anywhere in this file *before* that entry is now stale and will not resolve — the whole
  history was rewritten to scrub personal info before a public push, which changed every commit's
  hash. Trust `git log --oneline` over any hash written earlier in this document; hashes from that
  point on (starting with this entry, `9be284f`) are current.
- **Known environment quirk:** `cargo test --lib` occasionally hits a transient Windows linker error
  (`LNK1104: cannot open file ...claudehud-*.exe`), seen in both Task 10 and Task 11's runs. An
  immediate retry with no code changes always passes. Likely a stale file handle (antivirus scan or a
  lingering process) rather than a real bug — note this for any future task so nobody chases a ghost.
- **Environment note:** Rust 1.98.1 installed via `winget install Rustlang.Rustup`. Cargo bin is
  `%USERPROFILE%\.cargo\bin`; `setx` added it to the user PATH for new sessions, but the
  *current* shell environment does not see it (harness shells don't source `.bash_profile` and don't
  re-read the registry). Every bash command in this session that calls `cargo`/`rustc`/`rustup` must
  start with `export PATH="$PATH:$USERPROFILE/.cargo/bin"` (or use the full path). This
  applies to sub-agents too — tell each implementer explicitly.

### How to resume after a pause

1. Read this file, then `docs/plans/claudehud/README.md`.
2. Run `git status` and `git log --oneline -5`. If they disagree with the table below, trust git and
   fix this file first. For example, if a task's commit exists but is not marked done, mark it done.
3. If the current task is marked **in progress**, reread its task file. Its ticked checkboxes (`- [x]`)
   show how far it got. Check the working tree against those steps before continuing; never redo a
   step whose output is already committed.
4. Continue with **Next action**.

## Tasks

Status: `todo` · `in progress` · `review` · `done` · `blocked`

| # | Task | Implementer model | Status | Commit | Notes |
|---|---|---|---|---|---|
| 1 | Toolchain + scaffold | haiku | done | 17c3d0b, 71bfe0c | verified independently: tests pass, clippy clean, exe 208 KB |
| 2 | Spike: waiting signal + usage shape | main session (scripts) + user (live checks) | done | f890b27 | decision: proceed as specified, with adjustments for Tasks 5 and 7 |
| 3 | Model, time, format | sonnet | done | 4a2870e | verified independently: 20/20 tests pass, clippy clean; one deviation (`.is_multiple_of()` instead of `% 3 == 0`, clippy-forced, behavior identical) |
| 4 | Registry collector | haiku | done | 3996fab (merge 75ec391) | verified independently after merge |
| 5 | Transcript tail | haiku | done | 99a663f (merge 13dab9a) | verified independently after merge |
| 6 | Sub-agents | haiku | done | 1671c07 (merge 54f0f17) | had its own stub `tail.rs`/`transcript.rs` to compile in isolation; discarded in favor of Task 5's real versions during merge |
| 7 | Credentials, plan, usage | **sonnet** (escalated from haiku) | done | ce8cac4 (merge 1da4dc0) | added `spend`-object parsing (preferred over `extra_usage`), verified via `tests/usage_fixtures.rs` against the real live fixture |
| 8 | Status parser | haiku | done | efa02af (merge 6e64f91) | verified independently after merge |
| 9 | fold(), latch, fixtures, golden | sonnet | done | 92c9db6 | verified independently: 83 tests pass (incl. all 21 golden fixtures), clippy clean; trivial import-placement deviation (`SessionStatus` moved into the test module, matching `latch.rs`'s existing pattern, to satisfy clippy's unused-import gate) |
| 10 | Tooltip + icon pixels | sonnet | done | d3d0bac | verified independently: 84 lib tests + all integration tests pass, clippy clean; trivial `#[cfg(test)]`-gated import addition (types the given test code needs that the given top-level `use` line omitted) |
| 11 | Settings, geometry, hover | haiku | done | 407a6ae | verified independently: 105 lib tests + all integration tests pass, clippy and fmt clean; no code deviations (only cargo fmt line-wrapping) |
| 12 | Panel layout | sonnet | done | c360f3a | verified independently: 117 lib tests (incl. all 12 panel tests) + all integration tests pass, clippy clean; trivial `#[cfg(test)]`-gated import addition, same pattern as Tasks 10/11 |
| 13 | Collector + fetch + schedule | sonnet | done | 644814a | verified independently: 143 tests total pass (incl. all 12 collect tests, both flagged-tricky ones), clippy and fmt clean; trivial `#[allow(clippy::type_complexity)]` on a test-fake field, same pattern as prior tasks |
| 14 | App icon asset | haiku | done | 89ea91d, eda9ba0 (crop fix) | **coordinator independently re-verified after the fix, not just trusting the sub-agent's report**: re-extracted both PNG frames directly from the committed `.ico` and viewed them myself — 256px shows the full tile, creature centered, all 4 orbit spheres, clean corners; 16px shows the correct simplified tile. Also independently re-ran `cargo test` (117+ passing), clippy, and `cargo build --release` (431 KB, under budget) myself. |
| 15 | Platform services (Win32) | sonnet | done | 3a9558e | verified independently: 125 lib tests (1 ignored) + all integration tests pass, clippy/fmt clean, live WinHTTP status test passed, scratch registry key confirmed gone, real Run autostart key confirmed untouched. No real deviations — the task file's two flagged windows-0.61 API changes (`from_win32`→`from_thread`, `WinHttpOpenRequest`'s accept-types param) turned out not to apply to the pinned 0.61.3; the verbatim code compiled clean on the first try. |
| 16 | Strip + tray + loop | sonnet | done | 4237a26 | **coordinator independently re-ran the app and took my own screenshots**, not just trusting the sub-agent's report: started `claudehud.exe` myself with the yellow-waiting fixture and confirmed a yellow strip at the top of the screen; edited the fixture live (waiting→idle) and confirmed the strip changed to amber within ~2s (matches `QuotaWarn` outranking `Working` once the yellow reason clears, since this fixture's usage is already at 92%); restored the fixture via `git checkout`; force-killed the process and confirmed no `claudehud.exe` left running. Did not personally reproduce the tray-icon screenshot (it defaults to the taskbar overflow, which the task file itself treats as expected, not a failure) — accepted the implementer's UI-Automation-based verification for that part (it matched the icon's accessible Name to the exact expected tooltip string and confirmed badge color by pixel-sampling a zoomed capture). Two clippy-driven deviations (`chunks_exact_mut`→`as_chunks_mut`, one `#[allow(clippy::manual_dangling_ptr)]` with a comment explaining why the suggested fix would break `MAKEINTRESOURCEW(1)` icon loading) and one `scripts/screenshot.ps1` fix (`CopyFromScreen` with `CaptureBlt` throws on this machine — a documented .NET limitation — replaced with a direct `BitBlt` P/Invoke). 125+ tests still pass, clippy/fmt clean. |
| 17 | Live wiring + worker | sonnet | done | e97fee5 | verified independently: 125+ lib tests + all integration tests pass, clippy/fmt clean, no `claudehud.exe` left running. **Coordinator also independently re-ran the live check**: started the exe myself with no fixture and confirmed a green strip on screen (this session busy), matching the implementer's report. No windows-0.61 signature fixes needed this time — verbatim code compiled clean. Of the task file's 6 Step-4 manual checks, only the safe "green while busy" one was done (by both the implementer and me); the other 5 (kill a live session mid-turn, second permission-prompt session, close all sessions, disable Wi-Fi, wait-for-idle) were deliberately **not** delegated — held back to do with the user directly since they touch other live sessions/the real network. |
| 18 | Panel window (Direct2D) | sonnet | done | 0150405 | **coordinator independently re-verified visually, not just trusting the report**: rebuilt, ran the `team_mockup.json` and `red_quota_spent.json` fixtures myself, and confirmed both panels render correctly (header/usage/sessions/sub-agents/footer for the first; red banner + 100% red meter for the second) — screenshots matched the implementer's description in full detail. Along the way found a real environmental quirk worth recording (see decisions log): my first click attempt (simulated cursor + `mouse_event`) silently failed because a maximized window's Windows-11 "title bar scaffolding" hit-tested ahead of the topmost strip window across the entire top edge of the screen; worked around it by posting `WM_LBUTTONUP` directly to the strip's `HWND` (found via `FindWindow`), which is unaffected by hit-testing order. No windows-0.61.3 signature *spelling* fixes needed here, but one real deviation: `windows::Foundation::Numerics::Vector2` isn't reachable through any public path in this crate version (confirmed against crate source) — worked around with a macro that obtains a `Vector2` via `D2D1_ELLIPSE::default().point` rather than adding `windows-numerics` as an explicit new dependency (respects the no-new-crates-without-asking rule). Escalation to opus was authorized but not needed — sonnet handled it in one pass. 125+ tests pass, clippy/fmt clean, no `claudehud.exe` left running. |
| 19 | Menu + system integration | sonnet | done | 7ebfe05 | verified independently: 151 tests pass, clippy/fmt clean, no `claudehud.exe` left running, real `Run` autostart key confirmed untouched, test-generated settings.json cleaned up. One signature fix (`WM_MOUSEHOVER`/`WM_MOUSELEAVE` import path, same as Task 18). Step 3's full 12-item manual checklist deliberately deferred to `docs/manual-qa-pending.md` items 9-19 — this task's own file writes every one of them as "ask the user". |
| 20 | Smoke test, budgets, checklist | haiku | done | 322f55b, 5e32648 | verified independently: smoke tests pass (re-ran myself), full `check.ps1` gate passes, budget script passes (95MB working set vs 110MB revised target). **Working-set budget was investigated and corrected, not just accepted or silently patched over** — see decisions log for the full finding (Intel GPU driver overhead + real panel-open steady-state, no leak, confirmed via loaded-module inspection and repeated sampling). |
| — | Final whole-branch review | opus | todo | | after Task 20 |

## Model and sub-agent policy

- **Confirm with the user before starting the next task.** When a task's implementer and reviewer
  finish, report the result and wait for the user's go-ahead before dispatching the next task's
  sub-agent. Do not chain tasks automatically.
- **Always delegate to sub-agents.** The main session coordinates, talks to the user and updates
  this file. It does not write task code itself unless a sub-agent is unavailable.
- **Pick the cheapest model that fits:**
  - **haiku**: tasks whose code is given verbatim in the plan and whose tests are mechanical (copy, run, commit), plus scripts, file moves and simple lookups.
  - **sonnet**: tasks with real logic, Win32 or Direct2D work that needs compiler-driven fixes, and per-task code review.
  - **opus**: only the final whole-branch review, or a task that has already failed twice on sonnet.
- **Escalation:** if an implementer fails a task's tests or build twice, re-dispatch the task one model
  up with the failure output attached. Record the escalation in Notes.
- **Reviews:** after each task a fresh reviewer checks spec compliance and code quality: haiku for
  the haiku tasks, sonnet otherwise. Fix what it finds before marking the task done.
- **Parallelism:** tasks run in order (later tasks import earlier types). Two exceptions:
  - Task 14 (icon) may run alongside Tasks 3–13.
  - Tasks 4–8 only depend on Task 3, so they may run in parallel once it is done. Each must add its own `pub mod` line to `src/collectors/mod.rs`; resolve that one-line conflict when merging.

## Decisions and deviations log

Newest last. Record anything a resumed session must know that is not already in the plan.

- 2026-09-25: App name is **ClaudeHUD** (renamed from Sidelight throughout).
- 2026-09-25: Execution is subagent-driven; cheapest suitable model per task (see policy above).
- 2026-09-25: User asked to be asked for confirmation after each task finishes, before the next one
  starts. Do not auto-chain tasks.
- 2026-09-25: Task 2 spike done live against Claude Code 2.1.282. Full results: `docs/spike-results.md`.
  Key findings: permission prompts and `AskUserQuestion` both show `status=waiting` in the registry
  and clear on answer; plan approval (`ExitPlanMode`) **never** does, confirmed with prompts held open
  1-4s — so Task 5 (transcript tail) is the *only* signal for plan-mode waiting, not a defensive extra.
  Live usage response (`enterprise` / `default_claude_zero`) had empty `limits[]` and all-null named
  windows — a zero-token-quota, spend-only account shape the plan didn't anticipate — plus a new
  `spend` top-level object (cleaner than `extra_usage`: has `percent`/`severity` directly) and a long
  tail of null/codenamed keys to ignore. Task 7 must handle both quota-based and spend-based accounts
  and prefer `spend` over `extra_usage` when both are present. Fixture saved at
  `fixtures/usage/live-20260925.json` for Task 7's tests.
- 2026-09-25: User chose to run Tasks 4-8 in parallel. Each dispatched in an isolated git worktree
  (`Agent` tool `isolation: "worktree"`) to avoid concurrent writes to `src/collectors/mod.rs`; the
  coordinator merges all 5 branches into `main` afterward.
- 2026-09-25: Task 7 escalated from haiku to sonnet before starting (not a failure-escalation): the
  task file's verbatim `parse_spend` only reads `extra_usage`, but the Task 2 spike found a newer
  `spend` top-level object that should be preferred when present. Implementing that preference is new
  logic, not verbatim copying, so it needs the sonnet-tier judgment call.
- 2026-09-25: Tasks 4-8 all finished and were merged into `main` in this order: 4 (registry) → 8
  (status) → 7 (credentials/usage) → 5 (transcript) → 6 (subagents). Each merge after the first hit
  an add/add conflict on `src/collectors/mod.rs` (every task created it independently in its own
  isolated worktree) — resolved by combining all the `pub mod` lines. Task 6's own worktree also had
  to invent stub `src/collectors/tail.rs`/`transcript.rs` files to compile without visibility into
  Task 5's work; those stubs were discarded in favor of Task 5's real implementations during the
  Task 6 merge (`git checkout --ours` on those two files). After all 5 merges: full `cargo test` (61
  lib + 10 integration, all passing), `cargo clippy --all-targets -- -D warnings` and `cargo fmt
  --check` all clean on the merged result. All 5 worktrees and their branches were then removed.
- 2026-09-25: **Lesson for future parallel task batches**: when dispatching N tasks in parallel
  worktrees that all modify the same shared file (e.g. `src/collectors/mod.rs`) or depend on each
  other's new modules, each dispatch prompt must explicitly tell the sub-agent to (a) create any
  shared scaffolding file itself with only its own task's addition (never assume an earlier parallel
  task's output is visible), and (b) NOT invent stub versions of another parallel task's module if it
  can be avoided — if a real dependency is missing, ask the coordinator rather than guessing, since
  fabricated stubs create extra merge work. This was caught here (Task 6's stubs) but cost a full
  round of correction messages plus manual `git checkout --ours` during merge.
- 2026-09-28: **Real Windows 11 quirk found during Task 18 verification, not a ClaudeHUD bug**: when a
  window on the same monitor is maximized, Windows 11's DWM-owned "title bar scaffolding" overlay
  (window class `TITLE_BAR_SCAFFOLDING_WINDOW_CLASS`, used for the Snap Layout hover affordance) claims
  mouse hit-testing across the *entire top edge of the screen*, ahead of even `WS_EX_TOPMOST` windows —
  confirmed with `WindowFromPoint` at multiple coordinates across the strip's span, all returning that
  class instead of `ClaudeHUDStrip`. This means a real user's mouse literally cannot hover/click the
  strip through normal input while any window is maximized on that monitor, even though the strip still
  *draws* on top and looks clickable. Worked around it for verification purposes only by posting
  `WM_LBUTTONUP` straight to the strip's `HWND`; that is a test technique, not a fix — **the underlying
  reachability problem is unresolved in the shipped app**. Not blocking Task 18 itself, since the
  panel's rendering, layout and hover *logic* are all independently verified correct once a click
  reaches the window.
- 2026-09-28: **Resolved** (with the user): confirmed via a controlled test (a plain maximized WinForms
  window shows no interference at all) that the scaffolding overlay is specific to apps using Windows
  11's newer immersive title bar, not a property of maximized windows generally. Decision: document as
  a known OS-level limitation (now in `PLAN-CLAUDEHUD.md` §2.4) rather than attempt a code fix — no
  topmost window can override shell-privileged hit-testing, and the tray icon is a working fallback.
  `docs/manual-qa-pending.md` item 8 marked done.
- 2026-09-28: **Task 20's working-set budget was wrong, and fixing it took two rounds — both driven by
  actually measuring, not accepting a claim.** The haiku implementer reported 54.5MB vs the original
  40MB target and, correctly, did not silently patch the number — it flagged the overage as an open
  issue with code-review evidence that `Renderer`/`Surface` reuse was already correct. First
  independent check: reproduced 54MB as a real *baseline* (panel never opened) and traced it via
  `Get-Process -Module` to Intel GPU driver DLLs (`igc64.dll` 83MB, `igd10umt64xe.dll` 38MB mapped size)
  — confirmed not ClaudeHUD's own allocations. Asked the user how to handle it; they chose to raise the
  target. **But the first revision (60MB) was itself wrong** — it was based only on the no-panel
  baseline, when the budget script's own stated methodology is to measure *with the panel opened once*.
  Actually running that scenario (posting `WM_LBUTTONUP` to the strip's `HWND` mid-script, same
  technique as Tasks 18-19) showed ~95MB, not 54MB. Verified this wasn't a leak by sampling every 2-5s
  for over a minute (flat at 94.7MB) and across 6 repeated open/close cycles (94.7→95.0MB, noise, not
  growth). Final target: **110MB** in `PLAN-CLAUDEHUD.md` §8/§9, `scripts/budget.ps1`, and
  `docs/manual-checklist.md`, all updated together and re-verified end-to-end (`scripts/budget.ps1`
  now reports "Within budget" at 95MB/110MB). Commit `5e32648`.
- 2026-09-28: **Lesson**: when correcting a measured target/budget, reproduce the *exact* scenario the
  original check describes (here: "with the panel shown once", not idle) before picking a new number —
  a plausible-looking fix based on a partial measurement can itself be wrong, and the tell is usually
  right there in the check's own stated methodology.
- 2026-09-28: **Task 14's icon crop was wrong, and the implementer's own visual check missed it.**
  `scripts/make-icon.ps1`'s crop rectangle `(590, 135, 820, 820)` (copied verbatim from the task file)
  assumes a 2000px-wide source image, but the actual `assets/ClaudeHUD_icon.jpg` is **2816×1536**. The
  mismatch (~1.408x) cropped mostly blank canvas plus a corner of the tile, cutting off most of the
  creature and 3 of the 4 orbiting spheres. The haiku implementer reported "no fringe, looks correct"
  for this — its own visual check either wasn't done carefully or wasn't compared against the source
  artwork. The coordinator caught this only by independently re-extracting and viewing the frames
  during review. Corrected rectangle (scaled by 2816/2000 = 1.408): `(831, 190, 1155, 1155)` —
  verified by test-cropping and viewing before committing to the fix.
- 2026-09-28: **Lesson for future visual-check tasks**: when a task's acceptance criteria include "view
  the output and confirm it looks right," don't take the sub-agent's description at face value —
  independently re-derive and view the artifact yourself (or in a fresh reviewer pass) before marking
  the task done, the same way code output gets independently re-run. A sub-agent's "I looked and it's
  fine" is a claim, not evidence, exactly like its test-pass claims.

## Post-plan bug fixes (found by the user during manual testing, after all 20 tasks were done)

- 2026-09-28: **Bug 1 — pin icon, fixed and independently verified.** `render.rs`'s `Op::Pin` drew a
  crude circle+crossbar approximation instead of the mockup's actual pushpin SVG
  (`claudehud-mockup.html` line 252) — a plan-authoring gap from Task 18, not an implementation
  mistake. Fixed with real Direct2D path geometry tracing the exact SVG path (two open/hollow
  polylines). Commit `7af854a`. **Coordinator independently re-verified**: rebuilt, opened the panel
  against `team_mockup.json`, screenshotted and zoomed into the pin button — confirmed a proper angled
  pushpin/thumbtack shape with a needle point, not the old circle-and-crossbar.
- 2026-09-28: **Bug 2 — left-edge monitor-adjacency suppression, removed and independently verified.**
  User's primary monitor sits to the right of a second monitor, so the primary's left edge is fully
  adjacent to it; `edge_borders_other_monitor()` correctly detected this and set
  `reveal_suppressed = true` (documented, intended behavior, §2.3) — but it made the strip completely
  unable to hover-reveal on that edge, a common real setup. Decided with the user to remove the
  suppression mechanism entirely, relying on the existing 250ms `TME_HOVER` dwell as sufficient
  protection against accidental cross-monitor reveals. Fully deleted (not disabled):
  `edge_borders_other_monitor()`, the `reveal_suppressed` field, its `app.rs` wiring, both tests that
  covered the old behavior, and the `PLAN-CLAUDEHUD.md` §2.3/§8 references. Commit `7af854a` (same
  commit as Bug 1). **Coordinator independently re-verified with a real hover, not a synthetic click**:
  set `edge:"left", monitor:"primary"` in `target\release\claudehud.settings.json`, confirmed via
  `GetWindowRect` the strip was genuinely at the previously-broken position (x=0, on this machine's
  primary/left-adjacent edge), moved the real cursor onto it with `SetCursorPos` and waited for
  Windows' own `TrackMouseEvent`/`WM_MOUSEHOVER` delivery (no synthetic messages posted) — the panel
  revealed correctly, showing live real session data. This is a deliberate spec change made after the
  original 20-task plan completed, not a bug in the plan's own terms — recorded here so a resumed
  session knows why that mechanism is gone.
- 2026-09-28: **Note for whoever resumes**: verifying Bug 2 required overwriting
  `target\release\claudehud.settings.json` with `edge:"left", monitor:"primary"` for the test. This file
  is gitignored and not part of the repo, but if the user runs the exe from this same build output
  location again, it will start with those settings rather than whatever they had before — not
  restored automatically, since there was no prior copy to restore from (it wasn't read before being
  overwritten). Harmless (the tray menu can change it back anytime), but worth knowing.

## Pre-publish privacy cleanup (2026-09-28)

Before pushing this repo to GitHub, the user asked for a full audit of personal/private information.
Findings and actions:

- **Git commit history showed the user's real name and work email** on every commit (from global git
  config, no local override). **Fixed**: rewrote all 63 commits' author/committer identity to a
  generic `ClaudeHUD <claudehud@users.noreply.github.com>` via `git filter-branch --env-filter` (no
  `git-filter-repo` available; `filter-branch` was fine for this repo's size — 63 commits, one branch,
  no tags). Removed the `refs/original/` backup ref filter-branch creates, expired the reflog, and ran
  `git gc --prune=now --aggressive` — verified the old commit objects are actually gone (not just
  unreferenced) by confirming `git cat-file -p <old-hash>` fails. Safe to do because nothing had been
  pushed anywhere yet (`git remote -v` was empty the whole time) — no one else had a copy of the old
  history.
- **Windows username** (`C:\Users\<name>\...`) appeared in 2 lines of this very file — genericized to
  `%USERPROFILE%`/`$USERPROFILE`.
- **A real machine hostname** appeared in `PLAN-CLAUDEHUD.md`, `docs/plans/claudehud/task-04-registry.md`,
  and the actual compiled `src/collectors/registry.rs` (both the doc comment and a test fixture/fake)
  — replaced with a generic `dev-machine`/`DEV-MACHINE` placeholder, preserving the original's
  deliberate case-mismatch (the test exercises case-insensitive domain comparison).
- **A path to another private project** of the user's was cited in `PLAN-CLAUDEHUD.md` and
  `task-07-credentials-usage.md` as "what a similar tool reads" — replaced with a generic description.
- **Two other private project/session names** appeared as example data in `docs/spike-results.md` —
  replaced with a generic description of what was observed.
- **Real Anthropic Enterprise billing figures** (the account's actual $ spend) were in
  `fixtures/usage/live-20260925.json`, used as a real-world regression fixture. Replaced with
  fabricated numbers of the same shape/percentage; updated the matching assertions in
  `tests/usage_fixtures.rs` (`live_20260925_is_spend_only_via_the_spend_object`) and the prose in
  `docs/spike-results.md`/`task-07-credentials-usage.md` that quoted the real figures. Re-ran the full
  test suite after the fixture change to confirm the updated assertions still pass.
- Verified with repeated `git grep` sweeps (current tree) and a full `git log -p --all` scan (entire
  history, for tokens/secrets specifically) before and after each fix. Nothing else found: no leaked
  OAuth tokens, no other names/emails, no other hostnames.
- Committed as `9be284f` ("chore: add README, redact personal paths/hostnames before public release"),
  *then* the history rewrite ran on top of that, so this commit's own hash also changed (see the
  "IMPORTANT" note in Resume Here above).

## Blockers

None yet.
