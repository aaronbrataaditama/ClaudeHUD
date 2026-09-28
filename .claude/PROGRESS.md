# ClaudeHUD implementation progress

Single source of truth for where the implementation stands. **Update this file after every task
step that changes state** (task started, task done, review verdict, blocker, decision), and commit it
with the task's commit. Anyone resuming, human or model, reads this file first.

- Plan: `docs/plans/claudehud/README.md` + `docs/plans/claudehud/task-NN-*.md`
- Spec: `PLAN-CLAUDEHUD.md` · Rules: `CLAUDE.md` · Mockup: `claudehud-mockup.html`
- Execution method: **subagent-driven** (`superpowers:subagent-driven-development`)

## Resume here

- **Current task:** Task 15 (in progress — dispatched to sonnet sub-agent). Real Win32 FFI: process
  probe, local time, WinHTTP client, registry/autostart/single-instance/fullscreen helpers, log.
- **Next action:** review Task 15 sub-agent's work when it finishes, report to user, wait for
  go-ahead before Task 16 (the first visible milestone).
- **Branch:** `main` at `9ed1cbb`.
- **Waiting on user:** nothing right now
- **Known environment quirk:** `cargo test --lib` occasionally hits a transient Windows linker error
  (`LNK1104: cannot open file ...claudehud-*.exe`), seen in both Task 10 and Task 11's runs. An
  immediate retry with no code changes always passes. Likely a stale file handle (antivirus scan or a
  lingering process) rather than a real bug — note this for any future task so nobody chases a ghost.
- **Environment note:** Rust 1.98.1 installed via `winget install Rustlang.Rustup`. Cargo bin is
  `C:\Users\AaronBrataAditama\.cargo\bin`; `setx` added it to the user PATH for new sessions, but the
  *current* shell environment does not see it (harness shells don't source `.bash_profile` and don't
  re-read the registry). Every bash command in this session that calls `cargo`/`rustc`/`rustup` must
  start with `export PATH="$PATH:/c/Users/AaronBrataAditama/.cargo/bin"` (or use the full path). This
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
| 15 | Platform services (Win32) | sonnet | in progress | | dispatched to sonnet sub-agent |
| 16 | Strip + tray + loop | sonnet | todo | | first visible milestone; user checks the screen |
| 17 | Live wiring + worker | sonnet | todo | | user checks against real sessions |
| 18 | Panel window (Direct2D) | sonnet (escalate to opus if stuck) | todo | | hardest Win32 task; user checks against the mockup |
| 19 | Menu + system integration | sonnet | todo | | user runs the checklist in Step 3 |
| 20 | Smoke test, budgets, checklist | haiku | todo | | final gate |
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

## Blockers

None yet.
