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
        plan_label(
            self.subscription_type.as_deref(),
            self.rate_limit_tier.as_deref(),
        )
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
    let raw: RawFile = serde_json::from_str(strip_bom(text)).map_err(|e| {
        format!(
            "credentials file unreadable (line {}, column {})",
            e.line(),
            e.column()
        )
    })?;
    let o = raw.oauth;
    Ok(Credentials {
        token: o
            .as_ref()
            .and_then(|o| o.access_token.clone())
            .filter(|t| !t.is_empty())
            .map(Secret::new),
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

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRET","refreshToken":"sk-ant-ort01-SECRET","expiresAt":1790323000000,"scopes":["user:inference"],"subscriptionType":"team","rateLimitTier":"default_claude_max_5x"}}"#;

    #[test]
    fn reads_token_expiry_and_plan() {
        let c = parse_credentials(FILE).unwrap();
        assert_eq!(
            c.token.as_ref().map(Secret::expose),
            Some("sk-ant-oat01-SECRET")
        );
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
        assert_eq!(
            plan_label(Some("max"), Some("default_claude_max_20x")).as_deref(),
            Some("Max 20x")
        );
        assert_eq!(
            plan_label(Some("max"), Some("default_claude_max_5x")).as_deref(),
            Some("Max 5x")
        );
        assert_eq!(plan_label(Some("max"), None).as_deref(), Some("Max"));
        assert_eq!(
            plan_label(Some("team"), Some("default_claude_max_5x")).as_deref(),
            Some("Team · Max 5x")
        );
        assert_eq!(plan_label(Some("team"), None).as_deref(), Some("Team"));
        assert_eq!(
            plan_label(Some("Enterprise"), Some("whatever")).as_deref(),
            Some("Enterprise")
        );
        assert_eq!(
            plan_label(Some("startup"), Some("tier_x")).as_deref(),
            Some("tier_x")
        );
        assert_eq!(
            plan_label(Some("startup"), None).as_deref(),
            Some("startup")
        );
        assert_eq!(plan_label(None, None), None);
    }
}
