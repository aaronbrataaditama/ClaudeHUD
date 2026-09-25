//! Task 4: Read the tail of files to find recent events.

use std::path::Path;

pub const TAIL_BYTES: usize = 65_536;

pub fn read_tail(path: &Path, size: usize) -> Result<(String, bool), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let truncated = data.len() > size;
    let start = if truncated { data.len() - size } else { 0 };
    let text = String::from_utf8_lossy(&data[start..]).into_owned();
    Ok((text, truncated))
}

pub fn read_head(path: &Path, size: usize) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let end = std::cmp::min(size, data.len());
    let text = String::from_utf8_lossy(&data[..end]).into_owned();
    Ok(text)
}

pub fn complete_lines(text: &str, _truncated_start: bool) -> impl Iterator<Item = &str> {
    text.lines()
}
