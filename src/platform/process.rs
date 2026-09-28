use crate::collectors::registry::ProcessProbe;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

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
        WinProbe {
            domain: format!("win32:{}", hostname().to_lowercase()),
        }
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
        if GetComputerNameExW(
            ComputerNameDnsHostname,
            Some(PWSTR(buf.as_mut_ptr())),
            &mut len,
        )
        .is_err()
        {
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
            let (mut c, mut e, mut k, mut u) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
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
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        drop(child);
        assert!(
            p.creation_filetime(pid).is_none(),
            "exited process must not count as alive"
        );
    }

    #[test]
    fn domain_looks_like_claude_codes() {
        let d = WinProbe::new().pid_domain();
        assert!(d.starts_with("win32:") && d.len() > 6, "{d}");
        assert_eq!(d, d.to_lowercase());
    }
}
