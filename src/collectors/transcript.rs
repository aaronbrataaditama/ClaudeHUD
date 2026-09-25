//! Task 5: Parse the session transcript for model, tokens, and errors.

use serde_json::Value;

pub fn usage_input_total(usage: &Value) -> Option<u64> {
    let input = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(input + cache_read + cache_creation)
}
