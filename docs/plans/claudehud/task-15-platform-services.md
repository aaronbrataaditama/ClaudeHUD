# Task 15: Platform services (Windows)

**Goal:** Everything Windows-specific that is not a window: the process probe (`OpenProcess` + `GetProcessTimes`), local-time conversion, a WinHTTP client implementing `HttpGet`, single-instance mutex, HKCU registry helpers for autostart, fullscreen detection, opening URLs, dark context menus, well-known paths, plus a tiny size-capped warning log (platform-free).

**Spec:** §2 (WinHTTP, proxy, cert store), §2.4 (fullscreen), §3.1 (liveness), §6 (autostart path), §7 (single instance, log, redaction).

Read the README's **Notes for implementers** first: this code targets `windows` 0.61. Fix signature-level compiler complaints (e.g. `Option<…>` wrapping, `Result` vs `BOOL`, `Error::from_win32()` renamed `Error::from_thread()`) without changing behaviour.

**Files:**
- Create: `src/log.rs` (platform-free)
- Create: `src/platform/mod.rs`, `src/platform/wide.rs`, `src/platform/process.rs`, `src/platform/localtime.rs`, `src/platform/http.rs`, `src/platform/system.rs`
- Modify: `src/lib.rs` (add `pub mod log;` and `#[cfg(windows)] pub mod platform;`)

**Interfaces:**
- Consumes: `collectors::registry::ProcessProbe`, `collect::{HttpGet, HttpResponse, USER_AGENT}`, `timefmt::{LocalTime, utc_parts}`.
- Produces:
  - `log::{init(path: PathBuf), warn(msg: &str), append_to(&Path, &str), redact(msg: &str) -> String, MAX_LOG_BYTES}`
  - `platform::wide::{wide(&str) -> Vec<u16>}`
  - `platform::process::WinProbe` (`new()`, implements `ProcessProbe`)
  - `platform::localtime::local_parts(unix_s: i64) -> LocalTime`
  - `platform::http::WinHttp` (`new(user_agent) -> Result<WinHttp, String>`, implements `HttpGet`, `Send`)
  - `platform::system::{single_instance() -> Option<HANDLE>, RUN_KEY, reg_get_string(subkey, value) -> Option<String>, reg_set_string(subkey, value, data) -> bool, reg_delete_value(subkey, value) -> bool, sync_autostart(enabled: bool, exe: &Path), fullscreen_active() -> bool, open_url(&str), allow_dark_menus(), claude_dir() -> PathBuf, exe_dir() -> PathBuf, appdata_dir() -> Option<PathBuf>}`

---

## Part A: the log (platform-free)

- [ ] **Step 1: Failing tests**

`src/log.rs`, tests only:

```rust
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
        assert_eq!(redact("a.keynote b"), "a.keynote b", "only a real .key suffix");
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
        assert!(std::fs::read_to_string(&p).unwrap().contains("after rollover"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

Add `pub mod log;` to `src/lib.rs`.

- [ ] **Step 2: Implement**

Above the tests:

```rust
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
    if std::fs::metadata(path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {}", crate::timefmt::now_ms(), redact(msg));
    }
}

/// No-op until `init` is called.
pub fn warn(msg: &str) {
    if let Some(p) = PATH.get() {
        append_to(p, msg);
    }
}
```

Run: `cargo test --lib log::`
Expected: 2 passed.

## Part B: Windows services

- [ ] **Step 3: Module list and wide strings**

`src/platform/mod.rs`:

```rust
//! Everything that calls Win32. Nothing outside this module does.

pub mod http;
pub mod localtime;
pub mod process;
pub mod system;
pub mod wide;
```

`src/platform/wide.rs`:

```rust
/// NUL-terminated UTF-16 for `PCWSTR` parameters. Keep the Vec alive while the pointer is used.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
```

Add to `src/lib.rs`: `#[cfg(windows)] pub mod platform;`

- [ ] **Step 4: Process probe**

`src/platform/process.rs`:

```rust
use crate::collectors::registry::ProcessProbe;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW};
use windows::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

const STILL_ACTIVE: u32 = 259;

pub struct WinProbe {
    domain: String,
}

impl Default for WinProbe {
    fn default() -> Self {
        WinProbe::new()
    }
}

impl WinProbe {
    pub fn new() -> WinProbe {
        WinProbe { domain: format!("win32:{}", hostname().to_lowercase()) }
    }
}

fn hostname() -> String {
    unsafe {
        let mut len = 0u32;
        let _ = GetComputerNameExW(ComputerNameDnsHostname, None, &mut len);
        if len == 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize];
        if GetComputerNameExW(ComputerNameDnsHostname, Some(PWSTR(buf.as_mut_ptr())), &mut len).is_err() {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

impl ProcessProbe for WinProbe {
    fn creation_filetime(&self, pid: u32) -> Option<u64> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut code = 0u32;
            let alive = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE;
            let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            let times = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u);
            let _ = CloseHandle(h);
            if !alive || times.is_err() {
                return None;
            }
            Some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
        }
    }

    fn pid_domain(&self) -> String {
        self.domain.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive_and_exited_child_is_not() {
        let p = WinProbe::new();
        assert!(p.creation_filetime(std::process::id()).is_some());
        assert!(p.creation_filetime(0xFFFF_FFF0).is_none());
        let mut child = std::process::Command::new("cmd").args(["/c", "exit"]).spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        drop(child);
        assert!(p.creation_filetime(pid).is_none(), "exited process must not count as alive");
    }

    #[test]
    fn domain_looks_like_claude_codes() {
        let d = WinProbe::new().pid_domain();
        assert!(d.starts_with("win32:") && d.len() > 6, "{d}");
        assert_eq!(d, d.to_lowercase());
    }
}
```

- [ ] **Step 5: Local time**

`src/platform/localtime.rs`:

```rust
use crate::timefmt::{utc_parts, LocalTime};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// Unix seconds → local calendar parts, honouring the DST rules in force on that date.
pub fn local_parts(unix_s: i64) -> LocalTime {
    let ticks = (unix_s + 11_644_473_600).max(0) as u64 * 10_000_000;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut loc = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc).is_err() {
            return utc_parts(unix_s);
        }
    }
    LocalTime {
        year: loc.wYear as i32,
        month: loc.wMonth as u32,
        day: loc.wDay as u32,
        hour: loc.wHour as u32,
        minute: loc.wMinute as u32,
        weekday: loc.wDayOfWeek as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_is_within_a_day_of_utc() {
        let now = crate::timefmt::now_ms() / 1000;
        let l = local_parts(now);
        let u = utc_parts(now);
        let to_min = |t: &LocalTime| crate::timefmt::days_from_civil(t.year, t.month, t.day) * 1440 + (t.hour * 60 + t.minute) as i64;
        assert!((to_min(&l) - to_min(&u)).abs() <= 14 * 60, "offset within ±14 h");
        assert_eq!(l.minute % 15, u.minute % 15, "offsets are whole quarter hours");
    }
}
```

- [ ] **Step 6: WinHTTP client**

`src/platform/http.rs`:

```rust
//! HTTPS GET over WinHTTP: system proxy, OS certificate store, no TLS crate.
//! Error strings never include header values (they carry the OAuth token).

use super::wide::wide;
use crate::collect::{HttpGet, HttpResponse};
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_CUSTOM,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

const MAX_BODY: usize = 1024 * 1024;

pub struct WinHttp {
    session: *mut c_void,
}

// Created and used on the worker thread only; moved there once at startup.
unsafe impl Send for WinHttp {}

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn last_error(what: &str) -> String {
    format!("{what} failed: {}", windows::core::Error::from_win32())
}

impl WinHttp {
    pub fn new(user_agent: &str) -> Result<WinHttp, String> {
        let ua = wide(user_agent);
        let session = unsafe { WinHttpOpen(PCWSTR(ua.as_ptr()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0) };
        if session.is_null() {
            return Err(last_error("WinHttpOpen"));
        }
        unsafe {
            let _ = WinHttpSetTimeouts(session, 5_000, 10_000, 10_000, 20_000);
        }
        Ok(WinHttp { session })
    }
}

impl Drop for WinHttp {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.session);
        }
    }
}

unsafe fn query_status(req: *mut c_void) -> Result<u16, String> {
    let mut code = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    let mut index = 0u32;
    WinHttpQueryHeaders(
        req,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some(&mut code as *mut u32 as *mut c_void),
        &mut len,
        &mut index,
    )
    .map_err(|e| format!("status: {e}"))?;
    Ok(code as u16)
}

unsafe fn query_header(req: *mut c_void, name: &str) -> Option<String> {
    let name_w = wide(name);
    let mut buf = vec![0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let mut index = 0u32;
    WinHttpQueryHeaders(req, WINHTTP_QUERY_CUSTOM, PCWSTR(name_w.as_ptr()), Some(buf.as_mut_ptr() as *mut c_void), &mut len, &mut index).ok()?;
    Some(String::from_utf16_lossy(&buf[..(len as usize / 2).min(buf.len())]))
}

impl HttpGet for WinHttp {
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, String> {
        unsafe {
            let host_w = wide(host);
            let conn = Handle(WinHttpConnect(self.session, PCWSTR(host_w.as_ptr()), INTERNET_DEFAULT_HTTPS_PORT, 0));
            if conn.0.is_null() {
                return Err(last_error("connect"));
            }
            let verb = wide("GET");
            let object = wide(path);
            let req = Handle(WinHttpOpenRequest(
                conn.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(object.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ));
            if req.0.is_null() {
                return Err(last_error("open request"));
            }
            let header_text: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            let header_w: Vec<u16> = header_text.encode_utf16().collect();
            WinHttpSendRequest(req.0, Some(&header_w), None, 0, 0, 0).map_err(|e| format!("send: {e}"))?;
            WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| format!("receive: {e}"))?;
            let status = query_status(req.0)?;
            let retry_after_s = query_header(req.0, "Retry-After").and_then(|v| v.trim().parse().ok());
            let mut body = Vec::new();
            loop {
                let mut avail = 0u32;
                WinHttpQueryDataAvailable(req.0, &mut avail).map_err(|e| format!("read: {e}"))?;
                if avail == 0 {
                    break;
                }
                let mut buf = vec![0u8; avail as usize];
                let mut read = 0u32;
                WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, avail, &mut read).map_err(|e| format!("read: {e}"))?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..read as usize]);
                if body.len() > MAX_BODY {
                    return Err("response too large".to_string());
                }
            }
            Ok(HttpResponse { status, body: String::from_utf8_lossy(&body).into_owned(), retry_after_s })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits the real status page. Run manually: `cargo test --lib live_status_page -- --ignored`
    #[test]
    #[ignore]
    fn live_status_page() {
        let http = WinHttp::new(crate::collect::USER_AGENT).unwrap();
        let s = crate::collect::fetch_status(&http, 0).unwrap();
        assert!(!s.components.is_empty(), "{s:?}");
    }
}
```

- [ ] **Step 7: System helpers**

`src/platform/system.rs`:

```rust
use super::wide::wide;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, ShellExecuteW, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "ClaudeHUD";

/// Holds a named mutex for the life of the process. None = another instance runs.
pub fn single_instance() -> Option<HANDLE> {
    unsafe {
        let h = CreateMutexW(None, true, w!("Local\\ClaudeHUD.SingleInstance")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            return None;
        }
        Some(h)
    }
}

pub fn reg_get_string(subkey: &str, value: &str) -> Option<String> {
    let (k, v) = (wide(subkey), wide(value));
    unsafe {
        let mut bytes = 0u32;
        let r = RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut bytes));
        if r != ERROR_SUCCESS || bytes == 0 {
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(v.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut bytes),
        );
        if r != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

pub fn reg_set_string(subkey: &str, value: &str, data: &str) -> bool {
    let (k, v, d) = (wide(subkey), wide(value), wide(data));
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(v.as_ptr()),
            REG_SZ.0,
            Some(d.as_ptr() as *const c_void),
            (d.len() * 2) as u32,
        ) == ERROR_SUCCESS
    }
}

pub fn reg_delete_value(subkey: &str, value: &str) -> bool {
    let (k, v) = (wide(subkey), wide(value));
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr())) == ERROR_SUCCESS }
}

/// Makes HKCU\…\Run\ClaudeHUD match `enabled`, pointing at the exe's current path (§6).
pub fn sync_autostart(enabled: bool, exe: &Path) {
    let cmd = format!("\"{}\"", exe.display());
    let current = reg_get_string(RUN_KEY, RUN_VALUE);
    if enabled {
        if current.as_deref() != Some(cmd.as_str()) && !reg_set_string(RUN_KEY, RUN_VALUE, &cmd) {
            crate::log::warn("could not write the autostart registry value");
        }
    } else if current.is_some() && !reg_delete_value(RUN_KEY, RUN_VALUE) {
        crate::log::warn("could not remove the autostart registry value");
    }
}

/// A fullscreen app, D3D exclusive mode or presentation mode is active (§2.4).
pub fn fullscreen_active() -> bool {
    unsafe {
        matches!(
            SHQueryUserNotificationState(),
            Ok(s) if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE
        )
    }
}

pub fn open_url(url: &str) {
    let u = wide(url);
    unsafe {
        let _ = ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Undocumented uxtheme exports make native context menus follow the dark theme.
/// Ordinal 135 = SetPreferredAppMode(AllowDark = 1), 136 = FlushMenuThemes.
/// If either is missing, menus stay light; nothing else changes.
pub fn allow_dark_menus() {
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(f) = GetProcAddress(lib, PCSTR(135usize as *const u8)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_mode(1);
        }
        if let Some(f) = GetProcAddress(lib, PCSTR(136usize as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

/// `%CLAUDE_CONFIG_DIR%` if set, else `%USERPROFILE%\.claude`.
pub fn claude_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    home.join(".claude")
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn appdata_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = r"Software\ClaudeHUDTest";

    #[test]
    fn registry_round_trip_in_a_scratch_key() {
        assert!(reg_set_string(TEST_KEY, "Value", r#""C:\Tools\claudehud.exe""#));
        assert_eq!(reg_get_string(TEST_KEY, "Value").as_deref(), Some(r#""C:\Tools\claudehud.exe""#));
        assert!(reg_delete_value(TEST_KEY, "Value"));
        assert_eq!(reg_get_string(TEST_KEY, "Value"), None);
        unsafe {
            let k = wide(TEST_KEY);
            let _ = windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
        }
    }

    #[test]
    fn paths_are_sensible() {
        assert!(claude_dir().ends_with(".claude") || std::env::var_os("CLAUDE_CONFIG_DIR").is_some());
        assert!(exe_dir().is_dir());
    }

    #[test]
    fn fullscreen_query_does_not_crash() {
        let _ = fullscreen_active();
    }
}
```

Do **not** add a test that calls `sync_autostart`: it writes the real Run key.

- [ ] **Step 8: Build and run the Windows tests**

Run: `cargo test --lib platform::`
Expected: 6 passed, 1 ignored (`live_status_page`). Then run the ignored one once:
`cargo test --lib live_status_page -- --ignored`
Expected: passes (needs network; if behind a proxy that WinHTTP cannot auto-detect, report it rather than hard-coding a proxy).

- [ ] **Step 9: Lint and commit**

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/lib.rs src/log.rs src/platform
git commit -m "feat(platform): process probe, local time, WinHTTP, system helpers, log"
```
