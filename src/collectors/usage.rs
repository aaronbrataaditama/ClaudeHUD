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
    let v: Value = serde_json::from_str(strip_bom(text))
        .map_err(|e| format!("usage response is not JSON: {e}"))?;
    let obj = v.as_object().ok_or("usage response is not an object")?;
    let mut limits = parse_limits_array(obj);
    if limits.is_empty() {
        limits = parse_named_windows(obj);
    }
    Ok(Usage {
        limits,
        spend: parse_spend(obj),
    })
}

fn resets(v: &Value) -> Option<i64> {
    v.get("resets_at")
        .and_then(Value::as_str)
        .and_then(parse_iso8601)
}

fn parse_limits_array(obj: &Map<String, Value>) -> Vec<Limit> {
    let Some(rows) = obj.get("limits").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let pct = row.get("percent").and_then(Value::as_f64)?;
            let kind = row.get("kind").and_then(Value::as_str).unwrap_or("limit");
            let model = row
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str);
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
            Some(Limit {
                key,
                label,
                pct,
                resets_at: resets(row),
            })
        })
        .collect()
}

fn parse_named_windows(obj: &Map<String, Value>) -> Vec<Limit> {
    NAMED_WINDOWS
        .iter()
        .filter_map(|(key, label)| {
            let w = obj.get(*key).filter(|w| w.is_object())?;
            let pct = w.get("utilization").and_then(Value::as_f64)?;
            Some(Limit {
                key: key.to_string(),
                label: label.to_string(),
                pct,
                resets_at: resets(w),
            })
        })
        .collect()
}

/// Prefers the `spend` object (§3.5 live spike finding) over `extra_usage`; falls
/// back to `extra_usage` only if `spend` is absent, not an object, or its `used`
/// field is missing/null.
fn parse_spend(obj: &Map<String, Value>) -> Option<Spend> {
    parse_spend_object(obj).or_else(|| parse_extra_usage_spend(obj))
}

/// `"spend":{"used":{"amount_minor":N,"currency":"…"},"limit":{"amount_minor":N,"currency":"…"},"enabled":bool}`.
/// Amounts are minor units already (no `exponent` scaling needed: `amount_minor` is exact).
fn parse_spend_object(obj: &Map<String, Value>) -> Option<Spend> {
    let s = obj.get("spend").filter(|s| s.is_object())?;
    let used_minor = s.pointer("/used/amount_minor").and_then(Value::as_f64)?;
    let currency = s
        .get("currency")
        .and_then(Value::as_str)
        .or_else(|| s.pointer("/used/currency").and_then(Value::as_str))
        .or_else(|| s.pointer("/limit/currency").and_then(Value::as_str))
        .unwrap_or("USD")
        .to_string();
    Some(Spend {
        used_minor: used_minor.round() as i64,
        limit_minor: s
            .pointer("/limit/amount_minor")
            .and_then(Value::as_f64)
            .map(|l| l.round() as i64),
        currency,
        enabled: s.get("enabled").and_then(Value::as_bool).unwrap_or(true),
    })
}

fn parse_extra_usage_spend(obj: &Map<String, Value>) -> Option<Spend> {
    let e = obj.get("extra_usage").filter(|e| e.is_object())?;
    let used = e.get("used_credits").and_then(Value::as_f64)?;
    Some(Spend {
        used_minor: used.round() as i64,
        limit_minor: e
            .get("monthly_limit")
            .and_then(Value::as_f64)
            .map(|l| l.round() as i64),
        currency: e
            .get("currency")
            .and_then(Value::as_str)
            .unwrap_or("USD")
            .to_string(),
        enabled: e.get("is_enabled").and_then(Value::as_bool).unwrap_or(true),
    })
}

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
        assert_eq!(
            labels,
            vec![
                "Current session · 5h",
                "Weekly · all models",
                "Weekly · Opus"
            ]
        );
        assert_eq!(u.limits[0].key, "session");
        assert_eq!(u.limits[2].key, "weekly_scoped_opus");
    }

    #[test]
    fn enterprise_spend_only() {
        let u = parse_usage(ENTERPRISE).unwrap();
        assert!(u.limits.is_empty());
        let s = u.spend.unwrap();
        assert_eq!(
            (s.used_minor, s.limit_minor, s.currency.as_str(), s.enabled),
            (5_000, Some(60_000), "USD", true)
        );
    }

    #[test]
    fn spend_without_limit_and_defaults() {
        let u = parse_usage(
            r#"{"extra_usage":{"is_enabled":false,"monthly_limit":null,"used_credits":1234}}"#,
        )
        .unwrap();
        let s = u.spend.unwrap();
        assert_eq!(
            (s.limit_minor, s.currency.as_str(), s.enabled),
            (None, "USD", false)
        );
        assert!(
            parse_usage(r#"{"extra_usage":{"is_enabled":true,"used_credits":null}}"#)
                .unwrap()
                .spend
                .is_none()
        );
    }

    #[test]
    fn unknown_limit_kind_uses_label_or_kind() {
        let u = parse_usage(
            r#"{"limits":[{"kind":"daily_thing","percent":5},{"kind":"x","label":"Nice label","percent":6},{"kind":"no_percent"}]}"#,
        )
        .unwrap();
        let labels: Vec<&str> = u.limits.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(labels, vec!["daily thing", "Nice label"]);
    }

    #[test]
    fn rejects_non_objects_and_accepts_bom() {
        assert!(parse_usage("[]").is_err());
        assert!(parse_usage("<html>").is_err());
        assert_eq!(
            parse_usage(&format!("\u{feff}{NAMED}"))
                .unwrap()
                .limits
                .len(),
            3
        );
    }

    /// Live spike finding (§3.5, docs/spike-results.md): some accounts have both a
    /// `spend` object and an `extra_usage` object with different figures. `spend`
    /// must win.
    #[test]
    fn spend_object_wins_over_extra_usage_when_both_present() {
        let u = parse_usage(
            r#"{"spend":{"used":{"amount_minor":100,"currency":"USD"},"limit":{"amount_minor":200,"currency":"USD"},"enabled":true},"extra_usage":{"is_enabled":true,"monthly_limit":999999,"used_credits":999,"currency":"EUR"}}"#,
        )
        .unwrap();
        let s = u.spend.unwrap();
        assert_eq!(
            s.used_minor, 100,
            "spend.used.amount_minor should win over extra_usage.used_credits"
        );
        assert_eq!(s.limit_minor, Some(200));
        assert_eq!(s.currency, "USD");
        assert!(s.enabled);
    }

    /// `spend` present but its `used` is missing/null: fall through to `extra_usage`.
    #[test]
    fn falls_back_to_extra_usage_when_spend_used_is_missing() {
        let u = parse_usage(
            r#"{"spend":{"limit":{"amount_minor":200},"enabled":true},"extra_usage":{"is_enabled":true,"monthly_limit":999999,"used_credits":999,"currency":"EUR"}}"#,
        )
        .unwrap();
        let s = u.spend.unwrap();
        assert_eq!(s.used_minor, 999);
        assert_eq!(s.currency, "EUR");

        let u2 = parse_usage(
            r#"{"spend":{"used":null,"limit":{"amount_minor":200}},"extra_usage":{"is_enabled":true,"monthly_limit":999999,"used_credits":999,"currency":"EUR"}}"#,
        )
        .unwrap();
        assert_eq!(u2.spend.unwrap().used_minor, 999);
    }
}
