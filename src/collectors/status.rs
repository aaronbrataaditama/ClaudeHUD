//! `https://status.claude.com/api/v2/summary.json` (§3.6).

use super::strip_bom;
use crate::model::{Component, ComponentState, ServiceStatus};
use serde_json::Value;

/// Display order in the panel footer.
pub const WATCHED: [&str; 3] = ["Claude Code", "Claude API (api.anthropic.com)", "claude.ai"];
/// Components whose non-operational state turns the light red.
pub const RED_COMPONENTS: [&str; 2] = ["Claude Code", "Claude API (api.anthropic.com)"];

pub fn component_state(s: &str) -> ComponentState {
    match s {
        "operational" => ComponentState::Operational,
        "degraded_performance" => ComponentState::Degraded,
        "partial_outage" => ComponentState::PartialOutage,
        "major_outage" => ComponentState::MajorOutage,
        "under_maintenance" => ComponentState::Maintenance,
        _ => ComponentState::Other,
    }
}

pub fn parse_status(text: &str, now_ms: i64) -> Result<ServiceStatus, String> {
    let v: Value = serde_json::from_str(strip_bom(text))
        .map_err(|e| format!("status page is not JSON: {e}"))?;
    let comps = v
        .get("components")
        .and_then(Value::as_array)
        .ok_or("status page has no components")?;
    let components = WATCHED
        .iter()
        .filter_map(|want| {
            let c = comps
                .iter()
                .find(|c| c.get("name").and_then(Value::as_str) == Some(*want))?;
            let state = component_state(c.get("status").and_then(Value::as_str).unwrap_or(""));
            Some(Component {
                name: want.to_string(),
                state,
            })
        })
        .collect();
    let description = v
        .pointer("/status/description")
        .and_then(Value::as_str)
        .unwrap_or("Status unknown")
        .to_string();
    Ok(ServiceStatus {
        description,
        components,
        checked_at_ms: now_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ComponentState;

    const SUMMARY: &str = r#"{"page":{"id":"p","name":"Claude"},"status":{"indicator":"minor","description":"Partially Degraded Service"},"components":[{"id":"a","name":"claude.ai","status":"operational"},{"id":"x","name":"Console","status":"major_outage"},{"id":"b","name":"Claude API (api.anthropic.com)","status":"partial_outage"},{"id":"c","name":"Claude Code","status":"degraded_performance"}],"incidents":[]}"#;

    #[test]
    fn keeps_watched_components_in_fixed_order() {
        let s = parse_status(SUMMARY, 42).unwrap();
        let names: Vec<&str> = s.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Claude Code", "Claude API (api.anthropic.com)", "claude.ai"]
        );
        assert_eq!(s.components[0].state, ComponentState::Degraded);
        assert_eq!(s.components[1].state, ComponentState::PartialOutage);
        assert_eq!(s.components[2].state, ComponentState::Operational);
        assert_eq!(s.description, "Partially Degraded Service");
        assert_eq!(s.checked_at_ms, 42);
    }

    #[test]
    fn state_mapping() {
        assert_eq!(component_state("operational"), ComponentState::Operational);
        assert_eq!(
            component_state("degraded_performance"),
            ComponentState::Degraded
        );
        assert_eq!(
            component_state("partial_outage"),
            ComponentState::PartialOutage
        );
        assert_eq!(component_state("major_outage"), ComponentState::MajorOutage);
        assert_eq!(
            component_state("under_maintenance"),
            ComponentState::Maintenance
        );
        assert_eq!(component_state("sideways"), ComponentState::Other);
    }

    #[test]
    fn missing_components_are_skipped_not_invented() {
        let s = parse_status(r#"{"status":{"description":"All Systems Operational"},"components":[{"name":"claude.ai","status":"operational"}]}"#, 0).unwrap();
        assert_eq!(s.components.len(), 1);
    }

    #[test]
    fn rejects_non_statuspage_bodies() {
        assert!(parse_status("<html>maintenance</html>", 0).is_err());
        assert!(
            parse_status(r#"{"status":{}}"#, 0).is_err(),
            "no components array"
        );
    }

    #[test]
    fn red_components_are_a_subset_of_watched() {
        assert!(RED_COMPONENTS.iter().all(|r| WATCHED.contains(r)));
    }
}
