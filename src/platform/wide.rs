/// NUL-terminated UTF-16 for `PCWSTR` parameters. Keep the Vec alive while the pointer is used.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
