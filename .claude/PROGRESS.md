# ClaudeHUD implementation progress

Single source of truth for where the implementation stands. **Update this file after every task
step that changes state** (task started, task done, review verdict, blocker, decision), and commit it
with the task's commit. Anyone resuming, human or model, reads this file first.

- Plan: `docs/plans/claudehud/README.md` + `docs/plans/claudehud/task-NN-*.md`
- Spec: `PLAN-CLAUDEHUD.md` · Rules: `CLAUDE.md` · Mockup: `claudehud-mockup.html`
- Execution method: **subagent-driven** (`superpowers:subagent-driven-development`)

## Resume here

- **Current task:** Tasks 4-8 (in progress — dispatched together, each in its own isolated git
  worktree, per the user's choice to parallelize).
- **Next action:** when all 5 finish, merge each worktree branch into `main` (resolving the expected
  one-line conflict in `src/collectors/mod.rs` — each task adds its own `pub mod` line, keep all of
  them), verify the merged result builds and tests pass, report to user, wait for go-ahead before
  Task 9.
- **Branch:** `main` (4 commits: `17c3d0b`, `71bfe0c`, `f890b27`, `4a2870e`) + 5 pending worktree
  branches for Tasks 4-8, not yet merged.
- **Waiting on user:** nothing right now
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
| 4 | Registry collector | haiku | in progress | | running in parallel (isolated worktree) |
| 5 | Transcript tail | haiku | in progress | | running in parallel (isolated worktree) |
| 6 | Sub-agents | haiku | in progress | | running in parallel (isolated worktree) |
| 7 | Credentials, plan, usage | **sonnet** (escalated from haiku) | in progress | | running in parallel (isolated worktree); escalated because it needs new `spend`-object parsing logic beyond the task file's verbatim code (see decisions log) |
| 8 | Status parser | haiku | in progress | | running in parallel (isolated worktree) |
| 9 | fold(), latch, fixtures, golden | sonnet | todo | | core colour rules |
| 10 | Tooltip + icon pixels | sonnet | todo | | pixel maths |
| 11 | Settings, geometry, hover | haiku | todo | | |
| 12 | Panel layout | sonnet | todo | | largest pure module |
| 13 | Collector + fetch + schedule | sonnet | todo | | |
| 14 | App icon asset | haiku | todo | | PowerShell + visual check |
| 15 | Platform services (Win32) | sonnet | todo | | compile-driven signature fixes |
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

## Blockers

None yet.
