//! Every user-visible string that is built from data. Pure functions.

use crate::model::ComponentState;
use crate::timefmt::LocalTime;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// 950 → "950", 132_600 → "132.6k", 1_234_567 → "1.2M".
pub fn tokens(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let (v, unit) = if n < 999_950 {
        (n as f64 / 1_000.0, "k")
    } else {
        (n as f64 / 1_000_000.0, "M")
    };
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").unwrap_or(&s);
    format!("{s}{unit}")
}

/// Session running time: "<1m", "12m", "1h 04m", "2d 3h".
pub fn uptime(ms: i64) -> String {
    let m = ms.max(0) / 60_000;
    if m < 1 {
        "<1m".to_string()
    } else if m < 60 {
        format!("{m}m")
    } else if m < 24 * 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else {
        format!("{}d {}h", m / (24 * 60), (m / 60) % 24)
    }
}

/// Sub-agent elapsed time: "48s", "3m 05s", "1h 02m".
pub fn elapsed(ms: i64) -> String {
    let s = ms.max(0) / 1_000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3_600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3_600, (s / 60) % 60)
    }
}

/// "just now", "2m ago", "3h ago", "2d ago".
pub fn ago(ms: i64) -> String {
    let m = ms.max(0) / 60_000;
    if m < 1 {
        "just now".to_string()
    } else if m < 60 {
        format!("{m}m ago")
    } else if m < 24 * 60 {
        format!("{}h ago", m / 60)
    } else {
        format!("{}d ago", m / (24 * 60))
    }
}

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Minor units → display. Assumes two decimal places (true for USD/EUR/GBP).
pub fn money(minor: i64, currency: &str) -> String {
    let a = minor.unsigned_abs();
    let (whole, cents) = (a / 100, a % 100);
    let num = if cents == 0 {
        group_thousands(whole)
    } else {
        format!("{}.{:02}", group_thousands(whole), cents)
    };
    let s = match currency.to_ascii_uppercase().as_str() {
        "USD" => format!("${num}"),
        "EUR" => format!("€{num}"),
        other => format!("{num} {other}"),
    };
    if minor < 0 {
        format!("-{s}")
    } else {
        s
    }
}

/// "resets 14:32" (same local day), "resets Mon 09:00" (within 7 days), else "resets 3 Oct".
pub fn reset_label(reset_s: i64, now_s: i64, local: &dyn Fn(i64) -> LocalTime) -> String {
    let r = local(reset_s);
    let n = local(now_s);
    if (r.year, r.month, r.day) == (n.year, n.month, n.day) {
        format!("resets {:02}:{:02}", r.hour, r.minute)
    } else if reset_s > now_s && reset_s - now_s < 7 * 86_400 {
        format!(
            "resets {} {:02}:{:02}",
            WEEKDAYS[(r.weekday % 7) as usize],
            r.hour,
            r.minute
        )
    } else {
        format!(
            "resets {} {}",
            r.day,
            MONTHS[(r.month.clamp(1, 12) - 1) as usize]
        )
    }
}

/// Monthly spend resets on the 1st of next month: "resets 1 Oct".
pub fn spend_reset_label(now_local: &LocalTime) -> String {
    let next = if now_local.month >= 12 {
        1
    } else {
        now_local.month + 1
    };
    format!("resets 1 {}", MONTHS[(next - 1) as usize])
}

/// Floors below 100 so "100%" only ever means spent.
pub fn pct_label(p: f64) -> String {
    let v = if p >= 100.0 {
        100.0
    } else {
        p.max(0.0).floor()
    };
    format!("{}%", v as i64)
}

/// Shortens a path to fit `max_w`, keeping as many leading segments and the
/// final segment as possible: `C:\Projects\…\ClaudeHUD`.
pub fn middle_ellipsis(path: &str, max_w: f32, measure: &dyn Fn(&str) -> f32) -> String {
    if measure(path) <= max_w {
        return path.to_string();
    }
    let sep = if path.contains('\\') { '\\' } else { '/' };
    let parts: Vec<&str> = path.split(sep).collect();
    let last = parts
        .iter()
        .rev()
        .find(|p| !p.is_empty())
        .copied()
        .unwrap_or(path);
    if parts.len() > 2 {
        for keep in (1..parts.len() - 1).rev() {
            let head = parts[..keep].join(&sep.to_string());
            let cand = format!("{head}{sep}…{sep}{last}");
            if measure(&cand) <= max_w {
                return cand;
            }
        }
    }
    let cand = format!("…{sep}{last}");
    if measure(&cand) <= max_w {
        return cand;
    }
    let chars: Vec<char> = last.chars().collect();
    for start in 0..chars.len() {
        let cand: String = std::iter::once('…')
            .chain(chars[start..].iter().copied())
            .collect();
        if measure(&cand) <= max_w {
            return cand;
        }
    }
    "…".to_string()
}

pub fn sentence_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// At most `max` chars; the last one becomes "…" when cut.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    s.chars().take(keep).chain(std::iter::once('…')).collect()
}

/// "5-hour limit", "Weekly limit", "Weekly Opus limit", else the label.
pub fn limit_noun(key: &str, label: &str) -> String {
    match key {
        "five_hour" | "session" => "5-hour limit".to_string(),
        "seven_day" | "weekly_all" => "Weekly limit".to_string(),
        k if k.contains("opus") => "Weekly Opus limit".to_string(),
        k if k.contains("sonnet") => "Weekly Sonnet limit".to_string(),
        _ => label.to_string(),
    }
}

/// Short tooltip names for the two headline windows.
pub fn short_limit(key: &str) -> Option<&'static str> {
    match key {
        "five_hour" | "session" => Some("5h"),
        "seven_day" | "weekly_all" => Some("7d"),
        _ => None,
    }
}

/// "Claude API (api.anthropic.com)" → "Claude API".
pub fn short_component(name: &str) -> String {
    match name.find(" (") {
        Some(i) if name.ends_with(')') => name[..i].to_string(),
        _ => name.to_string(),
    }
}

pub fn component_state_text(state: ComponentState) -> &'static str {
    match state {
        ComponentState::Operational => "operational",
        ComponentState::Degraded => "degraded performance",
        ComponentState::PartialOutage => "partial outage",
        ComponentState::MajorOutage => "major outage",
        ComponentState::Maintenance => "under maintenance",
        ComponentState::Other => "status unknown",
    }
}

/// "claude-opus-5" → "Opus 5", "claude-haiku-4-5-20251001" → "Haiku 4.5".
/// Unrecognised ids are returned unchanged.
pub fn model_name(id: &str) -> String {
    let core = id.split('[').next().unwrap_or(id);
    let rest = core.strip_prefix("claude-").unwrap_or(core);
    let parts: Vec<&str> = rest
        .split('-')
        .filter(|p| !(p.len() == 8 && p.chars().all(|c| c.is_ascii_digit())))
        .collect();
    match parts.first() {
        Some(f) if matches!(*f, "opus" | "sonnet" | "haiku" | "fable") => {
            let version = parts[1..].join(".");
            let fam = sentence_case(f);
            if version.is_empty() {
                fam
            } else {
                format!("{fam} {version}")
            }
        }
        _ => id.to_string(),
    }
}

/// Last non-empty path segment.
pub fn folder_name(path: &str) -> String {
    path.split(['\\', '/'])
        .rev()
        .find(|p| !p.is_empty())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timefmt::{parse_iso8601, utc_parts};

    #[test]
    fn token_counts() {
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(1_000), "1k");
        assert_eq!(tokens(9_100), "9.1k");
        assert_eq!(tokens(132_600), "132.6k");
        assert_eq!(tokens(999_999), "1M");
        assert_eq!(tokens(1_234_567), "1.2M");
    }

    #[test]
    fn durations() {
        assert_eq!(uptime(30_000), "<1m");
        assert_eq!(uptime(12 * 60_000), "12m");
        assert_eq!(uptime(64 * 60_000), "1h 04m");
        assert_eq!(uptime((2 * 24 + 3) * 3_600_000), "2d 3h");
        assert_eq!(uptime(-5), "<1m");
        assert_eq!(elapsed(48_000), "48s");
        assert_eq!(elapsed(185_000), "3m 05s");
        assert_eq!(elapsed(62 * 60_000), "1h 02m");
        assert_eq!(ago(10_000), "just now");
        assert_eq!(ago(125_000), "2m ago");
        assert_eq!(ago(3 * 3_600_000), "3h ago");
    }

    #[test]
    fn money_formats() {
        assert_eq!(money(5_000, "USD"), "$50");
        assert_eq!(money(54_625, "USD"), "$546.25");
        assert_eq!(money(120_000, "usd"), "$1,200");
        assert_eq!(money(5_000, "EUR"), "€50");
        assert_eq!(money(5_000, "GBP"), "50 GBP");
        assert_eq!(money(0, "USD"), "$0");
    }

    #[test]
    fn reset_labels() {
        let now = parse_iso8601("2026-09-25T10:00:00Z").unwrap(); // a Friday
        assert_eq!(
            reset_label(
                parse_iso8601("2026-09-25T14:32:00Z").unwrap(),
                now,
                &utc_parts
            ),
            "resets 14:32"
        );
        assert_eq!(
            reset_label(
                parse_iso8601("2026-09-28T09:00:00Z").unwrap(),
                now,
                &utc_parts
            ),
            "resets Mon 09:00"
        );
        assert_eq!(
            reset_label(
                parse_iso8601("2026-10-03T09:00:00Z").unwrap(),
                now,
                &utc_parts
            ),
            "resets 3 Oct"
        );
        assert_eq!(spend_reset_label(&utc_parts(now)), "resets 1 Oct");
        let dec = parse_iso8601("2026-12-10T10:00:00Z").unwrap();
        assert_eq!(spend_reset_label(&utc_parts(dec)), "resets 1 Jan");
    }

    #[test]
    fn percentages_never_round_up_to_100() {
        assert_eq!(pct_label(61.4), "61%");
        assert_eq!(pct_label(99.6), "99%");
        assert_eq!(pct_label(100.0), "100%");
        assert_eq!(pct_label(130.0), "100%");
        assert_eq!(pct_label(-3.0), "0%");
    }

    #[test]
    fn middle_ellipsis_keeps_head_and_leaf() {
        let m = |s: &str| s.chars().count() as f32 * 7.0;
        let p = r"C:\Projects\Personal\ClaudeHUD";
        assert_eq!(middle_ellipsis(p, 1000.0, &m), p);
        assert_eq!(middle_ellipsis(p, 180.0, &m), r"C:\Projects\…\ClaudeHUD");
        assert_eq!(middle_ellipsis(p, 100.0, &m), r"C:\…\ClaudeHUD");
        assert_eq!(middle_ellipsis(p, 90.0, &m), r"…\ClaudeHUD");
        assert_eq!(middle_ellipsis(p, 50.0, &m), "…udeHUD");
        assert_eq!(middle_ellipsis("/home/a/b/c/leaf", 70.0, &m), "/…/leaf");
    }

    #[test]
    fn small_helpers() {
        assert_eq!(
            sentence_case("approve the permission prompt"),
            "Approve the permission prompt"
        );
        assert_eq!(sentence_case(""), "");
        assert_eq!(truncate_chars("abcdef", 4), "abc…");
        assert_eq!(truncate_chars("abc", 4), "abc");
        assert_eq!(limit_noun("five_hour", "x"), "5-hour limit");
        assert_eq!(limit_noun("weekly_all", "x"), "Weekly limit");
        assert_eq!(limit_noun("seven_day_opus", "x"), "Weekly Opus limit");
        assert_eq!(limit_noun("mystery", "Mystery window"), "Mystery window");
        assert_eq!(short_limit("session"), Some("5h"));
        assert_eq!(short_limit("seven_day"), Some("7d"));
        assert_eq!(short_limit("seven_day_opus"), None);
        assert_eq!(
            short_component("Claude API (api.anthropic.com)"),
            "Claude API"
        );
        assert_eq!(short_component("claude.ai"), "claude.ai");
        assert_eq!(
            component_state_text(crate::model::ComponentState::PartialOutage),
            "partial outage"
        );
        assert_eq!(folder_name(r"C:\Projects\Personal\ClaudeHUD\"), "ClaudeHUD");
        assert_eq!(folder_name("/home/me/app"), "app");
    }

    #[test]
    fn model_names() {
        assert_eq!(model_name("claude-opus-5"), "Opus 5");
        assert_eq!(model_name("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(model_name("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(model_name("claude-opus-5[1m]"), "Opus 5");
        assert_eq!(model_name("gpt-9"), "gpt-9");
    }
}
