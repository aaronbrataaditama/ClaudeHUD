# Task 7: Credentials, plan label, usage parser

**Goal:** Read the OAuth token and plan from `~/.claude/.credentials.json` without ever exposing the token, map the plan to a label ("Team · Max 5x"), and parse the usage response into `Usage` (limits + spend), preferring `limits[]` and falling back to named windows.

**Spec:** §3.4, §3.5. If `docs/spike-results.md` (Task 2) recorded a different usage shape, add a test for that shape here before implementing.

**It did.** The live spike (`docs/spike-results.md`) found a zero-token-quota enterprise account
where `limits[]` is empty and every named window is `null` — spend-only accounts are real, not a
theoretical edge case. It also found a `spend` top-level object the plan didn't originally document:
`{"used":{"amount_minor":9948,"currency":"USD","exponent":2},"limit":{"amount_minor":60000,"currency":"USD","exponent":2},"percent":17,"severity":"normal","enabled":true,...}`.
This maps directly onto the `Spend` struct (`used.amount_minor` → `used_minor`, `limit.amount_minor`
→ `limit_minor`, `currency`, `enabled`) and should be **preferred over `extra_usage` when both are
present** — parse `spend` first, fall back to `extra_usage` only if `spend` is absent or null. The
real fixture is saved at `fixtures/usage/live-20260925.json`; add it as a test case in
`tests/usage_fixtures.rs` (this account's expected result: empty `limits`, all windows absent,
`Spend { used_minor: 9948, limit_minor: Some(60000), currency: "USD", enabled: true }`). The response
also has a long tail of null/codenamed top-level keys (e.g. `tangelo`, `nimbus_quill`) — confirm
`parse_usage` ignores unknown top-level keys without error (already required by CLAUDE.md
conventions, but this fixture is what actually exercises it).

Known shapes:
- Credentials: `{"claudeAiOauth":{"accessToken":"…","refreshToken":"…","expiresAt":1790323000000,"scopes":[…],"subscriptionType":"team","rateLimitTier":"default_claude_max_5x"}}`
- Usage, named windows (what `C:\Projects\Personal\AIUsage\Platform\ClaudeUsage.cs` reads): `{"five_hour":{"utilization":61.0,"resets_at":"2026-09-25T14:32:00.123+00:00"},"seven_day":{"utilization":88.0,"resets_at":"…"},"seven_day_opus":null,…}`. `utilization` is a percentage 0–100.
- Usage, `limits[]` (newer; from the Claude Code bundle's schema): `{"limits":[{"kind":"session","group":"session","percent":61,"resets_at":"…"},{"kind":"weekly_scoped","group":"weekly","percent":42,"resets_at":"…","scope":{"model":{"display_name":"Opus"}}}]}`. Classify on `kind`, never on a label.
- Spend: `"extra_usage":{"is_enabled":true,"monthly_limit":60000,"used_credits":5000,"utilization":8.3,"currency":"USD"}`, amounts **in minor units** (cents). `monthly_limit: null` means no limit. `extra_usage: null` means none.

**Files:**
- Create: `src/collectors/credentials.rs`, `src/collectors/usage.rs`
- Create: `tests/usage_fixtures.rs`
- Modify: `src/collectors/mod.rs` (add `pub mod credentials; pub mod usage;`)

**Interfaces:**
- Consumes: `model::{Usage, Limit, Spend}`, `timefmt::parse_iso8601`, `collectors::strip_bom`.
- Produces:
  - `credentials::Secret` (`new(impl Into<String>)`, `expose(&self) -> &str`; `Debug` prints `Secret(***)`)
  - `credentials::Credentials { token: Option<Secret>, expires_at_ms: Option<i64>, subscription_type: Option<String>, rate_limit_tier: Option<String> }` with `plan_label(&self) -> Option<String>` and `is_expired(&self, now_ms: i64) -> bool`
  - `credentials::parse_credentials(&str) -> Result<Credentials, String>` (error text never contains file content)
  - `credentials::plan_label(Option<&str>, Option<&str>) -> Option<String>`
  - `usage::parse_usage(&str) -> Result<Usage, String>`

---

- [ ] **Step 1: Write failing tests for credentials**

`src/collectors/credentials.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRET","refreshToken":"sk-ant-ort01-SECRET","expiresAt":1790323000000,"scopes":["user:inference"],"subscriptionType":"team","rateLimitTier":"default_claude_max_5x"}}"#;

    #[test]
    fn reads_token_expiry_and_plan() {
        let c = parse_credentials(FILE).unwrap();
        assert_eq!(c.token.as_ref().map(Secret::expose), Some("sk-ant-oat01-SECRET"));
        assert_eq!(c.expires_at_ms, Some(1_790_323_000_000));
        assert_eq!(c.plan_label().as_deref(), Some("Team · Max 5x"));
        assert!(!c.is_expired(1_790_322_999_000));
        assert!(c.is_expired(1_790_323_000_000));
    }

    #[test]
    fn debug_output_never_contains_the_token() {
        let c = parse_credentials(FILE).unwrap();
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("SECRET"), "{dbg}");
        assert!(dbg.contains("Secret(***)"));
    }

    #[test]
    fn parse_errors_never_echo_content() {
        let bad = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRET","expiresAt":"soon"}}"#;
        let err = parse_credentials(bad).unwrap_err();
        assert!(!err.contains("SECRET") && !err.contains("soon"), "{err}");
    }

    #[test]
    fn missing_oauth_block_means_no_token() {
        let c = parse_credentials("{}").unwrap();
        assert!(c.token.is_none());
        assert!(c.is_expired(0), "no token counts as expired");
    }

    #[test]
    fn plan_labels() {
        assert_eq!(plan_label(Some("pro"), None).as_deref(), Some("Pro"));
        assert_eq!(plan_label(Some("max"), Some("default_claude_max_20x")).as_deref(), Some("Max 20x"));
        assert_eq!(plan_label(Some("max"), Some("default_claude_max_5x")).as_deref(), Some("Max 5x"));
        assert_eq!(plan_label(Some("max"), None).as_deref(), Some("Max"));
        assert_eq!(plan_label(Some("team"), Some("default_claude_max_5x")).as_deref(), Some("Team · Max 5x"));
        assert_eq!(plan_label(Some("team"), None).as_deref(), Some("Team"));
        assert_eq!(plan_label(Some("Enterprise"), Some("whatever")).as_deref(), Some("Enterprise"));
        assert_eq!(plan_label(Some("startup"), Some("tier_x")).as_deref(), Some("tier_x"));
        assert_eq!(plan_label(Some("startup"), None).as_deref(), Some("startup"));
        assert_eq!(plan_label(None, None), None);
    }
}
```

- [ ] **Step 2: Implement credentials**

Above its tests:

```rust
//! `~/.claude/.credentials.json`. Re-read on every poll; Claude Code rewrites it
//! when it refreshes the token (§3.5). ClaudeHUD never refreshes, logs or shows the token.

use super::strip_bom;
use serde::Deserialize;
use std::fmt;

/// A string that cannot be printed by accident.
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Secret {
        Secret(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[derive(Debug)]
pub struct Credentials {
    pub token: Option<Secret>,
    pub expires_at_ms: Option<i64>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

impl Credentials {
    pub fn plan_label(&self) -> Option<String> {
        plan_label(self.subscription_type.as_deref(), self.rate_limit_tier.as_deref())
    }

    /// True when there is no token or its expiry has passed.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        match (&self.token, self.expires_at_ms) {
            (None, _) => true,
            (Some(_), Some(exp)) => now_ms >= exp,
            (Some(_), None) => false,
        }
    }
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<RawOauth>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOauth {
    access_token: Option<String>,
    expires_at: Option<f64>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

pub fn parse_credentials(text: &str) -> Result<Credentials, String> {
    // serde's messages can quote values; never forward them.
    let raw: RawFile = serde_json::from_str(strip_bom(text))
        .map_err(|e| format!("credentials file unreadable (line {}, column {})", e.line(), e.column()))?;
    let o = raw.oauth;
    Ok(Credentials {
        token: o.as_ref().and_then(|o| o.access_token.clone()).filter(|t| !t.is_empty()).map(Secret::new),
        expires_at_ms: o.as_ref().and_then(|o| o.expires_at).map(|v| v as i64),
        subscription_type: o.as_ref().and_then(|o| o.subscription_type.clone()),
        rate_limit_tier: o.as_ref().and_then(|o| o.rate_limit_tier.clone()),
    })
}

/// §3.4 plan label.
pub fn plan_label(subscription: Option<&str>, tier: Option<&str>) -> Option<String> {
    let max = tier.and_then(|t| {
        if t.contains("max_20x") {
            Some("Max 20x")
        } else if t.contains("max_5x") {
            Some("Max 5x")
        } else {
            None
        }
    });
    match subscription.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("pro") => Some("Pro".to_string()),
        Some("max") => Some(max.unwrap_or("Max").to_string()),
        Some("team") => Some(match max {
            Some(m) => format!("Team · {m}"),
            None => "Team".to_string(),
        }),
        Some("enterprise") => Some("Enterprise".to_string()),
        Some("free") => Some("Free".to_string()),
        Some(_) => Some(tier.or(subscription).unwrap_or_default().to_string()),
        None => tier.map(str::to_string),
    }
}
```

Add `pub mod credentials;` to `src/collectors/mod.rs`.

- [ ] **Step 3: Run credentials tests**

Run: `cargo test --lib collectors::credentials`
Expected: 5 passed.

- [ ] **Step 4: Write failing tests for usage**

`src/collectors/usage.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::timefmt::parse_iso8601;

    const NAMED: &str = r#"{"five_hour":{"utilization":61.0,"resets_at":"2026-09-25T14:32:00.123+00:00"},"seven_day":{"utilization":88,"resets_at":"2026-09-28T09:00:00+00:00"},"seven_day_opus":{"utilization":42.5,"resets_at":null},"seven_day_sonnet":null,"extra_usage":null}"#;
    const LIMITS: &str = r#"{"limits":[{"kind":"session","group":"session","percent":61,"resets_at":"2026-09-25T14:32:00Z"},{"kind":"weekly_all","group":"weekly","percent":88,"resets_at":"2026-09-28T09:00:00Z"},{"kind":"weekly_scoped","group":"weekly","percent":42,"resets_at":"2026-09-28T09:00:00Z","scope":{"model":{"display_name":"Opus"}}}],"five_hour":{"utilization":1}}"#;
    const ENTERPRISE: &str = r#"{"five_hour":null,"seven_day":null,"extra_usage":{"is_enabled":true,"monthly_limit":60000,"used_credits":5000.0,"utilization":8.3,"currency":"USD"}}"#;

    #[test]
    fn named_windows_in_fixed_order_skipping_nulls() {
        let u = parse_usage(NAMED).unwrap();
        let keys: Vec<&str> = u.limits.iter().map(|l| l.key.as_str()).collect();
        assert_eq!(keys, vec!["five_hour", "seven_day", "seven_day_opus"]);
        assert_eq!(u.limits[0].label, "Current session · 5h");
        assert_eq!(u.limits[1].label, "Weekly · all models");
        assert_eq!(u.limits[2].label, "Weekly · Opus");
        assert_eq!(u.limits[0].pct, 61.0);
        assert_eq!(u.limits[0].resets_at, parse_iso8601("2026-09-25T14:32:00Z"));
        assert_eq!(u.limits[2].resets_at, None);
        assert!(u.spend.is_none());
    }

    #[test]
    fn limits_array_wins_over_named_windows() {
        let u = parse_usage(LIMITS).unwrap();
        let labels: Vec<&str> = u.limits.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(labels, vec!["Current session · 5h", "Weekly · all models", "Weekly · Opus"]);
        assert_eq!(u.limits[0].key, "session");
        assert_eq!(u.limits[2].key, "weekly_scoped_opus");
    }

    #[test]
    fn enterprise_spend_only() {
        let u = parse_usage(ENTERPRISE).unwrap();
        assert!(u.limits.is_empty());
        let s = u.spend.unwrap();
        assert_eq!((s.used_minor, s.limit_minor, s.currency.as_str(), s.enabled), (5_000, Some(60_000), "USD", true));
    }

    #[test]
    fn spend_without_limit_and_defaults() {
        let u = parse_usage(r#"{"extra_usage":{"is_enabled":false,"monthly_limit":null,"used_credits":1234}}"#).unwrap();
        let s = u.spend.unwrap();
        assert_eq!((s.limit_minor, s.currency.as_str(), s.enabled), (None, "USD", false));
        assert!(parse_usage(r#"{"extra_usage":{"is_enabled":true,"used_credits":null}}"#).unwrap().spend.is_none());
    }

    #[test]
    fn unknown_limit_kind_uses_label_or_kind() {
        let u = parse_usage(r#"{"limits":[{"kind":"daily_thing","percent":5},{"kind":"x","label":"Nice label","percent":6},{"kind":"no_percent"}]}"#).unwrap();
        let labels: Vec<&str> = u.limits.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(labels, vec!["daily thing", "Nice label"]);
    }

    #[test]
    fn rejects_non_objects_and_accepts_bom() {
        assert!(parse_usage("[]").is_err());
        assert!(parse_usage("<html>").is_err());
        assert_eq!(parse_usage(&format!("\u{feff}{NAMED}")).unwrap().limits.len(), 3);
    }
}
```

- [ ] **Step 5: Implement usage**

Above its tests:

```rust
//! Response of `GET https://api.anthropic.com/api/oauth/usage` (§3.5).

use super::strip_bom;
use crate::model::{Limit, Spend, Usage};
use crate::timefmt::parse_iso8601;
use serde_json::{Map, Value};

const NAMED_WINDOWS: [(&str, &str); 5] = [
    ("five_hour", "Current session · 5h"),
    ("seven_day", "Weekly · all models"),
    ("seven_day_opus", "Weekly · Opus"),
    ("seven_day_sonnet", "Weekly · Sonnet"),
    ("seven_day_oauth_apps", "Weekly · OAuth apps"),
];

pub fn parse_usage(text: &str) -> Result<Usage, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| format!("usage response is not JSON: {e}"))?;
    let obj = v.as_object().ok_or("usage response is not an object")?;
    let mut limits = parse_limits_array(obj);
    if limits.is_empty() {
        limits = parse_named_windows(obj);
    }
    Ok(Usage { limits, spend: parse_spend(obj) })
}

fn resets(v: &Value) -> Option<i64> {
    v.get("resets_at").and_then(Value::as_str).and_then(parse_iso8601)
}

fn parse_limits_array(obj: &Map<String, Value>) -> Vec<Limit> {
    let Some(rows) = obj.get("limits").and_then(Value::as_array) else { return Vec::new() };
    rows.iter()
        .filter_map(|row| {
            let pct = row.get("percent").and_then(Value::as_f64)?;
            let kind = row.get("kind").and_then(Value::as_str).unwrap_or("limit");
            let model = row.pointer("/scope/model/display_name").and_then(Value::as_str);
            let (key, label) = match (kind, model) {
                ("session", _) => ("session".to_string(), "Current session · 5h".to_string()),
                ("weekly_all", _) => ("weekly_all".to_string(), "Weekly · all models".to_string()),
                ("weekly_scoped", Some(m)) => (
                    format!("weekly_scoped_{}", m.to_lowercase().replace(' ', "_")),
                    format!("Weekly · {m}"),
                ),
                (k, _) => (
                    k.to_string(),
                    row.get("label")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| k.replace('_', " ")),
                ),
            };
            Some(Limit { key, label, pct, resets_at: resets(row) })
        })
        .collect()
}

fn parse_named_windows(obj: &Map<String, Value>) -> Vec<Limit> {
    NAMED_WINDOWS
        .iter()
        .filter_map(|(key, label)| {
            let w = obj.get(*key).filter(|w| w.is_object())?;
            let pct = w.get("utilization").and_then(Value::as_f64)?;
            Some(Limit { key: key.to_string(), label: label.to_string(), pct, resets_at: resets(w) })
        })
        .collect()
}

fn parse_spend(obj: &Map<String, Value>) -> Option<Spend> {
    let e = obj.get("extra_usage").filter(|e| e.is_object())?;
    let used = e.get("used_credits").and_then(Value::as_f64)?;
    Some(Spend {
        used_minor: used.round() as i64,
        limit_minor: e.get("monthly_limit").and_then(Value::as_f64).map(|l| l.round() as i64),
        currency: e.get("currency").and_then(Value::as_str).unwrap_or("USD").to_string(),
        enabled: e.get("is_enabled").and_then(Value::as_bool).unwrap_or(true),
    })
}
```

Add `pub mod usage;` to `src/collectors/mod.rs`.

- [ ] **Step 6: Run usage tests**

Run: `cargo test --lib collectors::usage`
Expected: 6 passed.

- [ ] **Step 7: Guard against the real response shape**

`tests/usage_fixtures.rs` parses every file saved by the Task 2 spike:

```rust
use claudehud::collectors::usage::parse_usage;

#[test]
fn every_saved_live_response_parses() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/usage");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let u = parse_usage(&text).unwrap_or_else(|err| panic!("{}: {err}", p.display()));
        assert!(!u.limits.is_empty() || u.spend.is_some(), "{}: no limits and no spend", p.display());
    }
}
```

Run: `cargo test --test usage_fixtures`
Expected: 1 passed. If a saved live response fails, the parser is wrong for the real shape: fix the parser (add a unit test for that shape first), not the fixture.

- [ ] **Step 8: Lint and commit**

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/collectors tests/usage_fixtures.rs
git commit -m "feat(usage): credentials, plan labels and usage/spend parsing"
```
