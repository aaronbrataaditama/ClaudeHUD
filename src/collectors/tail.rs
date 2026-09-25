//! Bounded reads of append-only JSONL files that another process is writing.
//! `File::open` on Windows shares read/write/delete, so Claude Code is never blocked.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub const TAIL_BYTES: u64 = 65_536;

/// The last `max` bytes. `truncated_start` is true when the read began mid-file,
/// so the first line is probably partial.
pub fn read_tail(path: &Path, max: u64) -> io::Result<(String, bool)> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf)?;
    Ok((String::from_utf8_lossy(&buf).into_owned(), start > 0))
}

/// The first `max` bytes (the last line may be partial).
pub fn read_head(path: &Path, max: u64) -> io::Result<String> {
    let f = File::open(path)?;
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Non-empty lines without `\r`. Drops the first line when `truncated_start`.
/// A torn final line is still yielded; callers skip lines that fail to parse.
pub fn complete_lines(text: &str, truncated_start: bool) -> impl Iterator<Item = &str> {
    let mut it = text.split('\n');
    if truncated_start {
        it.next();
    }
    it.map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_partial_first_line_only_when_truncated() {
        let t = "partial}\n{\"a\":1}\r\n\n{\"b\":2}";
        assert_eq!(
            complete_lines(t, true).collect::<Vec<_>>(),
            vec!["{\"a\":1}", "{\"b\":2}"]
        );
        assert_eq!(complete_lines(t, false).count(), 3);
    }
}
