//! Launches the real exe and inspects its windows. Needs a desktop session, so it
//! is ignored by default. Run: `cargo test --test smoke -- --ignored --test-threads=1`
#![cfg(windows)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowLongPtrW, IsWindowVisible, GWL_EXSTYLE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn launch() -> Running {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/snapshots/green_working.json"
    );
    Running(
        Command::new(env!("CARGO_BIN_EXE_claudehud"))
            .env("CLAUDEHUD_FIXTURE", fixture)
            .spawn()
            .expect("spawn claudehud"),
    )
}

fn wait_for_strip() -> HWND {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(h) = unsafe { FindWindowW(w!("ClaudeHUDStrip"), None) } {
            if !h.is_invalid() && unsafe { IsWindowVisible(h) }.as_bool() {
                return h;
            }
        }
        assert!(
            Instant::now() < deadline,
            "strip window never became visible"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore]
fn strip_has_the_right_styles_and_never_steals_focus() {
    let before = unsafe { GetForegroundWindow() };
    let _app = launch();
    let strip = wait_for_strip();
    let ex = unsafe { GetWindowLongPtrW(strip, GWL_EXSTYLE) } as u32;
    for (flag, name) in [
        (WS_EX_LAYERED.0, "WS_EX_LAYERED"),
        (WS_EX_TOOLWINDOW.0, "WS_EX_TOOLWINDOW"),
        (WS_EX_NOACTIVATE.0, "WS_EX_NOACTIVATE"),
        (WS_EX_TOPMOST.0, "WS_EX_TOPMOST"),
    ] {
        assert!(ex & flag != 0, "strip is missing {name} (ex style {ex:#x})");
    }
    std::thread::sleep(Duration::from_secs(2)); // a few ticks
    assert_eq!(
        unsafe { GetForegroundWindow() },
        before,
        "ClaudeHUD changed the foreground window"
    );
}

#[test]
#[ignore]
fn second_instance_exits_immediately() {
    let _first = launch();
    wait_for_strip();
    let mut second = launch();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if second.0.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "second instance is still running"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
