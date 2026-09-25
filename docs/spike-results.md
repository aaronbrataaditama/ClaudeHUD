# Spike results (Task 2), 2026-09-25, Claude Code 2.1.282

| Trigger | status seen | waitingFor | latency after prompt appeared | cleared on answer? |
|---|---|---|---|---|
| Permission prompt (Bash) | `waiting` | `permission prompt` | sub-second (limited by the 250ms poll interval; observed on 4 separate occasions) | yes, back to `busy` |
| AskUserQuestion | `waiting` | `input needed` | sub-second (same poll limit; observed twice) | yes, back to `busy` |
| Plan approval (ExitPlanMode) | **never `waiting`** | n/a | n/a | n/a |
| MCP elicitation | not tested (no MCP server configured that prompts) | | | |

- procStart matches process creation FILETIME: **yes** — every observed session showed `procStart=OK`
  across dozens of transitions and multiple sessions (`claudehud-5c`, `claudehud-ec`, `claudehud-1a`,
  `tests-6d`, `docassembler-69`, `ai-hacking-australia-summary`). No tolerance adjustment needed.
- Killed mid-turn: registry file kept? **yes**, status left as `busy` (user killed terminal B mid-turn;
  the file was not deleted and stayed at `busy` rather than updating).
- Usage response top-level keys: `five_hour, seven_day, seven_day_oauth_apps, seven_day_opus,
  seven_day_sonnet, seven_day_cowork, seven_day_omelette, tangelo, iguana_necktie,
  omelette_promotional, nimbus_quill, cinder_cove, copper_kite, brass_thimble, harbor_lantern,
  wattle_ember, amber_ladder, amber_cistern, juniper_tide, cedar_ember, amber_gauge, extra_usage,
  limits, spend, member_dashboard_available, seven_day_breakdown`
- `limits[]` present: yes, but **empty** (`[]`) for this account. Named windows present as keys but
  **all `null`** (`five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`, `seven_day_oauth_apps`).
- `extra_usage` present: yes. Shape: `{is_enabled, monthly_limit, used_credits, utilization, currency,
  decimal_places, disabled_reason, user_disabled, spend_limit_reached, credits_ever_enabled, daily,
  weekly}` — a superset of what the plan documented (extra fields, all optional/ignorable).
- Plan: subscriptionType = `enterprise`, rateLimitTier = `default_claude_zero`.

## Additional findings not anticipated by the plan

1. **A `spend` top-level object exists** that the plan never accounted for:
   `{used: {amount_minor, currency, exponent}, limit: {amount_minor, currency, exponent}, percent,
   severity, enabled, disabled_reason, cap: {money, credits: {amount_minor, exponent}}, balance,
   auto_reload, disclaimer, can_purchase_credits, can_toggle}`. It already carries a computed
   `percent` (17) and a `severity` enum (`"normal"`) that line up with `extra_usage.utilization`
   (16.58%) — this is a cleaner, more directly usable source for $ spend display than `extra_usage`.
2. **This account has neither `limits[]` nor any populated named window** — it is a zero-token-quota
   enterprise account billed purely by $ spend (`rateLimitTier=default_claude_zero`). This is a
   different shape than the Team/Max-5x case the plan was written around (that case has real
   `limits[]`/named-window data; this one has none). Task 7 must handle both: quota-based accounts
   (real `limits[]`/named windows, `extra_usage`/`spend` absent or zero) and spend-based accounts
   (quota fields empty/null, `extra_usage`/`spend` populated).
3. **A long tail of null/codenamed top-level keys** (`tangelo`, `iguana_necktie`,
   `omelette_promotional`, `cinder_cove`, `copper_kite`, `brass_thimble`, `harbor_lantern`,
   `wattle_ember`, `amber_ladder`, `amber_cistern`, `juniper_tide`, `cedar_ember`, `amber_gauge`) —
   these look like internal feature-flag/experiment codenames. One, `nimbus_quill`, is non-null with
   its own shape (`utilization`, `resets_at`, `limit_dollars`, `used_dollars`, `remaining_dollars`,
   `locked_reason`) but was all-zero/null for this account. Task 7's parser must ignore unknown
   top-level keys gracefully (already required by CLAUDE.md conventions) — this response is a good
   regression fixture for that.
4. **Plan approval (`ExitPlanMode`) never produced a `waiting`/`waitingFor` entry in the registry**,
   confirmed twice (prompts left open 1-2s and 3-4s — both well past the 250ms poll interval, so this
   is a genuine absence, not a missed capture). This matches the architecture already anticipated in
   §3.1: `AskUserQuestion` and `ExitPlanMode` were expected to need the transcript-tail fallback
   (Task 5) rather than the registry. In practice `AskUserQuestion` turned out to also surface via the
   registry (a bonus), but `ExitPlanMode` does not — so for plan-mode approval, the transcript-tail
   fallback in Task 5 is not a defensive extra, it is the **only** signal. Task 5 must specifically
   verify `ExitPlanMode` is detectable in the transcript when it is built.

## Decision

**Proceed as specified**, with two adjustments carried forward:

- Task 5 (transcript tail): treat `ExitPlanMode` detection as load-bearing, not defensive. Add a test
  using a transcript fixture containing an `ExitPlanMode` tool call awaiting a response, and verify it
  live against a real plan-approval prompt when that task is implemented.
- Task 7 (credentials, plan, usage): the parser must handle three usage shapes gracefully — (a)
  `limits[]` populated, (b) named windows populated, (c) neither populated but `spend`/`extra_usage`
  carry $ figures instead (this account's case). Prefer `spend` over `extra_usage` for $ display when
  both are present, since `spend` already provides `percent`/`severity`. All unknown top-level keys
  (the codenamed ones above) must be ignored without error. The saved fixture
  `fixtures/usage/live-20260925.json` is a real example of case (c) and should be used as a test
  fixture for Task 7.
