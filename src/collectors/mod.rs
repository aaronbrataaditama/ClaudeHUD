//! Collectors read `projects/*/` to produce facts for the snapshot.

pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

pub mod subagents;
pub mod tail;
pub mod transcript;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_utf8_bom() {
        assert_eq!(strip_bom("\u{FEFF}hello"), "hello");
        assert_eq!(strip_bom("hello"), "hello");
    }
}
