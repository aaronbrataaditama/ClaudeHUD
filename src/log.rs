//! Warnings-only log beside the settings file, 1 MB with one rollover (§7).
//! Paths ending in `.key` are redacted: those files hold capability tokens.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
static PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    let _ = PATH.set(path);
}

/// Replaces every whitespace-separated word ending in `.key` (optionally
/// followed by `:` `,` `)` `"`) with `[redacted].key`.
pub fn redact(msg: &str) -> String {
    msg.split(' ')
        .map(|word| {
            let core = word.trim_end_matches([':', ',', ')', '"']);
            if core.ends_with(".key") {
                format!("[redacted].key{}", &word[core.len()..])
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn append_to(path: &Path, msg: &str) {
    if std::fs::metadata(path)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{} {}", crate::timefmt::now_ms(), redact(msg));
    }
}

/// No-op until `init` is called.
pub fn warn(msg: &str) {
    if let Some(p) = PATH.get() {
        append_to(p, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_key_file_paths() {
        assert_eq!(
            redact(r"cannot read C:\Users\me\.claude\sessions\22488.4363ad5d.key: denied"),
            r"cannot read [redacted].key: denied"
        );
        assert_eq!(redact("nothing secret here"), "nothing secret here");
        assert_eq!(
            redact("a.keynote b"),
            "a.keynote b",
            "only a real .key suffix"
        );
    }

    #[test]
    fn writes_and_rolls_over() {
        let dir = std::env::temp_dir().join(format!("claudehud-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("claudehud.log");
        append_to(&p, "first");
        assert!(std::fs::read_to_string(&p).unwrap().contains("first"));
        let big = "x".repeat(MAX_LOG_BYTES as usize);
        append_to(&p, &big);
        append_to(&p, "after rollover");
        assert!(dir.join("claudehud.log.1").exists());
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("after rollover"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
