use super::wide::wide;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, ShellExecuteW, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
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
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(v.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        );
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
    unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(v.as_ptr()))
            == ERROR_SUCCESS
    }
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
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(u.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Undocumented uxtheme exports make native context menus follow the dark theme.
/// Ordinal 135 = SetPreferredAppMode(AllowDark = 1), 136 = FlushMenuThemes.
/// If either is missing, menus stay light; nothing else changes.
pub fn allow_dark_menus() {
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else {
            return;
        };
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
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default();
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
        assert!(reg_set_string(
            TEST_KEY,
            "Value",
            r#""C:\Tools\claudehud.exe""#
        ));
        assert_eq!(
            reg_get_string(TEST_KEY, "Value").as_deref(),
            Some(r#""C:\Tools\claudehud.exe""#)
        );
        assert!(reg_delete_value(TEST_KEY, "Value"));
        assert_eq!(reg_get_string(TEST_KEY, "Value"), None);
        unsafe {
            let k = wide(TEST_KEY);
            let _ = windows::Win32::System::Registry::RegDeleteKeyW(
                HKEY_CURRENT_USER,
                PCWSTR(k.as_ptr()),
            );
        }
    }

    #[test]
    fn paths_are_sensible() {
        assert!(
            claude_dir().ends_with(".claude") || std::env::var_os("CLAUDE_CONFIG_DIR").is_some()
        );
        assert!(exe_dir().is_dir());
    }

    #[test]
    fn fullscreen_query_does_not_crash() {
        let _ = fullscreen_active();
    }
}
