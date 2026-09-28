//! Small Win32 window helpers shared by the controller, strip and panel windows.

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetSystemMetrics, LoadCursorW, LoadIconW, RegisterClassExW, SetWindowPos,
    ShowWindow, HWND_TOPMOST, IDC_ARROW, SM_CXSMICON, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WNDCLASSEXW, WNDPROC, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub fn register_class(hinst: HINSTANCE, name: PCWSTR, proc: WNDPROC) -> bool {
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: proc,
            hInstance: hinst,
            lpszClassName: name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // resource id 1 = assets/claudehud.ico (Task 14); MAKEINTRESOURCEW-style integer
            // resource id encoded as a pointer value, not a real dangling pointer.
            #[allow(clippy::manual_dangling_ptr)]
            hIcon: LoadIconW(Some(hinst), PCWSTR(1usize as *const u16)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc) != 0
    }
}

/// Hidden top-level window: owns timers and the tray icon and receives broadcast
/// messages (display, power, settings changes). Never shown.
pub fn create_controller(hinst: HINSTANCE, class: PCWSTR) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            w!("ClaudeHUD"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )
    }
}

/// Per-pixel-alpha popup that never activates, never shows in the taskbar or Alt-Tab,
/// and stays on top (§2.1).
pub fn create_layered(hinst: HINSTANCE, class: PCWSTR) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinst),
            None,
        )
    }
}

pub fn set_topmost(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

pub fn show_noactivate(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    set_topmost(hwnd);
}

pub fn hide(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

pub fn small_icon_size() -> u32 {
    unsafe { GetSystemMetrics(SM_CXSMICON).max(16) as u32 }
}
