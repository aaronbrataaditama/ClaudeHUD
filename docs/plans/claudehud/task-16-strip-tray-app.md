# Task 16: Strip window, tray icon, message loop (fixture-driven)

**Goal:** The first visible milestone. `claudehud.exe` starts, shows the 4 px strip in the colour `fold()` gives for a `CLAUDEHUD_FIXTURE` snapshot, and shows the creature tray icon with the right badge and tooltip. Right-click → Exit works. No live data yet (Task 17), no panel yet (Task 18).

**Spec:** §2.1 (strip window styles; off = hidden), §2.4 (per-monitor DPI; the manifest from Task 1 already declares PerMonitorV2), §5 (tray), §5.1 (tray icon), §7 (single instance).

Read the README's **Notes for implementers** first (windows 0.61 signatures, `try_borrow_mut` in wndprocs).

**Files:**
- Create: `src/platform/win.rs`, `src/platform/layered.rs`, `src/platform/monitors.rs`, `src/platform/tray.rs`, `src/platform/app.rs`
- Create: `scripts/screenshot.ps1`
- Modify: `src/platform/mod.rs`, `src/main.rs`

**Interfaces:**
- Consumes: `geometry::{pick_monitor, strip_rect, MonitorInfo, Rect}`, `icon::{render_strip, render_tray_icon, Badge}`, `tooltip::tooltip`, `state::fold`, `fixture::load_snapshot`, `settings::{load, settings_path, Settings}`, `platform::{system, localtime, wide}`, `log`.
- Produces (used by Tasks 17–19):
  - `platform::win::{register_class(HINSTANCE, PCWSTR, WNDPROC) -> bool, create_controller(HINSTANCE, PCWSTR) -> windows::core::Result<HWND>, create_layered(HINSTANCE, PCWSTR) -> windows::core::Result<HWND>, show_noactivate(HWND), set_topmost(HWND), hide(HWND), small_icon_size() -> u32}`
  - `platform::layered::{present(HWND, Rect, &[u8], u8) -> bool, present_dc(HWND, Rect, HDC, u8) -> bool, move_and_fade(HWND, i32, i32, u8) -> bool}`
  - `platform::monitors::enumerate() -> Vec<MonitorInfo>`
  - `platform::tray::{Tray, WM_TRAY}`; `Tray::{new(HWND), update(&mut self, size: u32, bgra: &[u8], tip: &str), notify(&mut self, title: &str, text: &str), reset(&mut self), remove(&mut self)}`
  - `platform::app::run()`

---

- [x] **Step 1: Window helpers**

`src/platform/win.rs`:

```rust
//! Small Win32 window helpers shared by the controller, strip and panel windows.

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetSystemMetrics, LoadCursorW, LoadIconW, RegisterClassExW, SetWindowPos, ShowWindow,
    HWND_TOPMOST, IDC_ARROW, SM_CXSMICON, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SW_HIDE,
    SW_SHOWNOACTIVATE, WNDCLASSEXW, WNDPROC, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

pub fn register_class(hinst: HINSTANCE, name: PCWSTR, proc: WNDPROC) -> bool {
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: proc,
            hInstance: hinst,
            lpszClassName: name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // resource id 1 = assets/claudehud.ico (Task 14)
            hIcon: LoadIconW(Some(hinst), PCWSTR(1usize as *const u16)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc) != 0
    }
}

/// Hidden top-level window: owns timers and the tray icon and receives broadcast
/// messages (display, power, settings changes). Never shown.
pub fn create_controller(hinst: HINSTANCE, class: PCWSTR) -> Result<HWND> {
    unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, class, w!("ClaudeHUD"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None) }
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
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
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
```

- [x] **Step 2: Layered-window presentation**

`src/platform/layered.rs`:

```rust
//! Pushes premultiplied BGRA pixels to a WS_EX_LAYERED window with UpdateLayeredWindow.

use crate::geometry::Rect;
use std::ffi::c_void;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 }
}

pub fn dib_info(w: i32, h: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Shows `bgra` (w*h*4 premultiplied bytes) at `r`, with overall opacity `alpha`.
pub fn present(hwnd: HWND, r: Rect, bgra: &[u8], alpha: u8) -> bool {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bmi = dib_info(r.w, r.h);
        let ok = match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) => {
                let n = bgra.len().min((r.w * r.h * 4).max(0) as usize);
                std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, n);
                let old = SelectObject(mem, bmp.into());
                let ok = present_dc(hwnd, r, mem, alpha);
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                ok
            }
            Err(_) => false,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        ok
    }
}

/// Same, from a memory DC that already has a 32-bpp DIB selected (the panel renderer's).
pub fn present_dc(hwnd: HWND, r: Rect, dc: HDC, alpha: u8) -> bool {
    unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&POINT { x: r.x, y: r.y }),
            Some(&SIZE { cx: r.w, cy: r.h }),
            Some(dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend(alpha)),
            ULW_ALPHA,
        )
        .is_ok()
    }
}

/// Moves and fades an already-presented window without re-sending pixels (animation frames).
pub fn move_and_fade(hwnd: HWND, x: i32, y: i32, alpha: u8) -> bool {
    unsafe {
        UpdateLayeredWindow(hwnd, None, Some(&POINT { x, y }), None, None, None, COLORREF(0), Some(&blend(alpha)), ULW_ALPHA)
            .is_ok()
    }
}
```

- [x] **Step 3: Monitor enumeration**

`src/platform/monitors.rs`:

```rust
use crate::geometry::{MonitorInfo, Rect};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

// `BOOL` lives in windows::Win32::Foundation in some 0.6x releases and in windows::core in others.
use windows::core::BOOL;

fn rect(r: RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

unsafe extern "system" fn collect(hmon: HMONITOR, _hdc: HDC, _clip: *mut RECT, data: LPARAM) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<MonitorInfo>);
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let end = mi.szDevice.iter().position(|&c| c == 0).unwrap_or(mi.szDevice.len());
        let bounds = rect(mi.monitorInfo.rcMonitor);
        out.push(MonitorInfo {
            id: String::from_utf16_lossy(&mi.szDevice[..end]),
            name: format!("Display {} · {}×{}", out.len() + 1, bounds.w, bounds.h),
            primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            bounds,
            work: rect(mi.monitorInfo.rcWork),
            scale: dx as f32 / 96.0,
        });
    }
    true.into()
}

/// All monitors in physical pixels (the process is PerMonitorV2-aware via its manifest).
pub fn enumerate() -> Vec<MonitorInfo> {
    let mut out: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut out as *mut Vec<MonitorInfo> as isize));
    }
    out
}
```

- [x] **Step 4: Tray icon**

`src/platform/tray.rs`:

```rust
//! Shell_NotifyIcon wrapper (legacy callback mode: lParam carries the mouse message).

use super::layered::dib_info;
use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, DIB_RGB_COLORS};
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO, WM_APP};

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

/// Icons want straight (non-premultiplied) alpha.
fn unpremultiply(src: &[u8]) -> Vec<u8> {
    let mut out = src.to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

pub fn icon_from_bgra(size: u32, premultiplied: &[u8]) -> Option<HICON> {
    let straight = unpremultiply(premultiplied);
    let s = size as i32;
    unsafe {
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &dib_info(s, s), DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(straight.as_ptr(), bits as *mut u8, straight.len().min((s * s * 4) as usize));
        let mask_bytes = vec![0u8; (((s + 15) / 16) * 2 * s) as usize];
        let mask = CreateBitmap(s, s, 1, 1, Some(mask_bytes.as_ptr() as *const c_void));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let max = dst.len() - 1;
    for (i, c) in s.encode_utf16().take(max).enumerate() {
        dst[i] = c;
    }
}

pub struct Tray {
    hwnd: HWND,
    icon: Option<HICON>,
    added: bool,
    last: Option<(Vec<u8>, String)>,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        Tray { hwnd, icon: None, added: false, last: None }
    }

    fn base(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            ..Default::default()
        }
    }

    /// Adds or updates the icon. Cheap when nothing changed.
    pub fn update(&mut self, size: u32, bgra: &[u8], tip: &str) {
        if self.added && self.last.as_ref().is_some_and(|(p, t)| p == bgra && t == tip) {
            return;
        }
        let Some(icon) = icon_from_bgra(size, bgra) else { return };
        let mut nid = self.base();
        nid.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;
        copy_wide(&mut nid.szTip, tip);
        let ok = unsafe { Shell_NotifyIconW(if self.added { NIM_MODIFY } else { NIM_ADD }, &nid) }.as_bool();
        if ok {
            self.added = true;
            self.last = Some((bgra.to_vec(), tip.to_string()));
        }
        if let Some(old) = self.icon.replace(icon) {
            unsafe {
                let _ = DestroyIcon(old);
            }
        }
    }

    /// A one-off balloon notification (first run, panel unavailable).
    pub fn notify(&mut self, title: &str, text: &str) {
        if !self.added {
            return;
        }
        let mut nid = self.base();
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut nid.szInfoTitle, title);
        copy_wide(&mut nid.szInfo, text);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Explorer restarted (TaskbarCreated): the icon must be added again.
    pub fn reset(&mut self) {
        self.added = false;
        self.last = None;
    }

    pub fn remove(&mut self) {
        if self.added {
            let nid = self.base();
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            self.added = false;
        }
        if let Some(i) = self.icon.take() {
            unsafe {
                let _ = DestroyIcon(i);
            }
        }
    }
}
```

- [x] **Step 5: The app (Task 16 version)**

`src/platform/app.rs`. Task 17 replaces this whole file.

```rust
//! Message loop and wiring. Task 16 version: strip + tray driven by CLAUDEHUD_FIXTURE.

use super::layered;
use super::localtime::local_parts;
use super::monitors;
use super::system;
use super::tray::{Tray, WM_TRAY};
use super::win;
use crate::geometry::{self, MonitorInfo, Rect};
use crate::icon::{self, Badge};
use crate::model::{Colour, Light, Snapshot, DIM_ALPHA};
use crate::settings::{self, Settings};
use crate::{fixture, log, state, timefmt, tooltip};
use std::cell::RefCell;
use std::path::PathBuf;
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, PostMessageW, PostQuitMessage, RegisterWindowMessageW, SetForegroundWindow, SetTimer,
    TrackPopupMenu, TranslateMessage, MA_NOACTIVATE, MF_STRING, MSG, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_DESTROY, WM_MOUSEACTIVATE, WM_NULL, WM_RBUTTONUP, WM_TIMER,
};

const TIMER_TICK: usize = 1;
const CMD_EXIT: u32 = 199;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app unless the state is already borrowed (a re-entrant message).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

struct App {
    controller: HWND,
    strip: HWND,
    tray: Tray,
    taskbar_created: u32,
    settings: Settings,
    monitors: Vec<MonitorInfo>,
    fixture: Option<PathBuf>,
    snapshot: Snapshot,
    light: Light,
    shown_strip: Option<(Rect, Colour, bool)>,
}

pub fn run() {
    let Some(_instance) = system::single_instance() else { return };
    let settings_path = settings::settings_path(&system::exe_dir(), system::appdata_dir().as_deref());
    log::init(settings_path.with_file_name("claudehud.log"));
    let settings = settings::load(&settings_path);
    let hinst: HINSTANCE = match unsafe { GetModuleHandleW(None) } {
        Ok(m) => m.into(),
        Err(e) => {
            log::warn(&format!("GetModuleHandleW: {e}"));
            return;
        }
    };
    if !win::register_class(hinst, w!("ClaudeHUDController"), Some(controller_proc))
        || !win::register_class(hinst, w!("ClaudeHUDStrip"), Some(strip_proc))
    {
        log::warn("RegisterClassExW failed");
        return;
    }
    let (Ok(controller), Ok(strip)) = (
        win::create_controller(hinst, w!("ClaudeHUDController")),
        win::create_layered(hinst, w!("ClaudeHUDStrip")),
    ) else {
        log::warn("CreateWindowExW failed");
        return;
    };
    let app = App {
        controller,
        strip,
        tray: Tray::new(controller),
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        settings,
        monitors: monitors::enumerate(),
        fixture: std::env::var_os("CLAUDEHUD_FIXTURE").map(PathBuf::from),
        snapshot: Snapshot::default(),
        light: Light::off(),
        shown_strip: None,
    };
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    with_app(App::refresh);
    unsafe {
        SetTimer(Some(controller), TIMER_TICK, 1000, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    with_app(|a| a.tray.remove());
}

impl App {
    fn refresh(&mut self) {
        let now = timefmt::now_ms();
        self.snapshot = match &self.fixture {
            Some(p) => match fixture::load_snapshot(p) {
                Ok(mut s) => {
                    fixture::anchor(&mut s, now);
                    s
                }
                Err(e) => {
                    log::warn(&e);
                    Snapshot { now_ms: now, ..Snapshot::default() }
                }
            },
            None => Snapshot { now_ms: now, warn_percent: self.settings.warn_percent, ..Snapshot::default() },
        };
        self.light = state::fold(&self.snapshot, Some(&self.light));
        self.update_strip();
        self.update_tray();
    }

    fn monitor(&self) -> Option<MonitorInfo> {
        geometry::pick_monitor(&self.monitors, &self.settings.monitor).cloned()
    }

    fn update_strip(&mut self) {
        let Some(m) = self.monitor() else { return };
        if self.light.colour == Colour::Off {
            if self.shown_strip.take().is_some() {
                win::hide(self.strip);
            }
            return;
        }
        let r = geometry::strip_rect(&m, self.settings.edge);
        let key = (r, self.light.colour, self.light.dim);
        if self.shown_strip == Some(key) {
            win::set_topmost(self.strip); // another app's topmost window may have demoted us
            return;
        }
        let alpha = if self.light.dim { DIM_ALPHA } else { 1.0 };
        let px = icon::render_strip(r.w as u32, r.h as u32, self.light.colour.rgb(), alpha);
        if layered::present(self.strip, r, &px, 255) {
            win::show_noactivate(self.strip);
            self.shown_strip = Some(key);
        }
    }

    fn update_tray(&mut self) {
        let size = win::small_icon_size();
        let off = self.light.colour == Colour::Off;
        let alpha = if self.light.dim { DIM_ALPHA } else { 1.0 };
        let badge = (!off).then_some(Badge { rgb: self.light.colour.rgb(), alpha });
        let px = icon::render_tray_icon(size, badge, off);
        let tip = tooltip::tooltip(&self.light, &self.snapshot, &local_parts);
        self.tray.update(size, &px, &tip);
    }

    fn on_controller(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_TIMER if wp.0 == TIMER_TICK => self.refresh(),
            WM_TRAY => {
                if (lp.0 as u32) & 0xFFFF == WM_RBUTTONUP {
                    self.show_menu();
                }
            }
            m if self.taskbar_created != 0 && m == self.taskbar_created => {
                self.tray.reset();
                self.update_tray();
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn show_menu(&mut self) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT as usize, w!("Exit"));
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // Required so the menu closes when the user clicks elsewhere (documented tray quirk).
            let _ = SetForegroundWindow(self.controller);
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, Some(0), self.controller, None);
            let _ = PostMessageW(Some(self.controller), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            self.on_command(cmd.0 as u32);
        }
    }

    fn on_command(&mut self, cmd: u32) {
        if cmd == CMD_EXIT {
            unsafe {
                let _ = DestroyWindow(self.controller);
            }
        }
    }
}

extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(Some(r)) = with_app(|a| a.on_controller(msg, wp, lp)) {
        return r;
    }
    if msg == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

extern "system" fn strip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}
```

- [x] **Step 6: Wire modules and `main`**

`src/platform/mod.rs`, full content:

```rust
//! Everything that calls Win32. Nothing outside this module does.

pub mod app;
pub mod http;
pub mod layered;
pub mod localtime;
pub mod monitors;
pub mod process;
pub mod system;
pub mod tray;
pub mod wide;
pub mod win;
```

`src/main.rs`, full content:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    claudehud::platform::app::run();
    #[cfg(not(windows))]
    eprintln!("ClaudeHUD runs on Windows only.");
}
```

Run: `cargo build --release` and fix any signature-level compile errors (see README notes).
Expected: builds with no warnings.

- [x] **Step 7: Screenshot helper for verification**

`scripts/screenshot.ps1`:

```powershell
# Captures part of the screen (layered windows included) to a PNG for checking the UI.
#   -Region top   : 600x60 at the top centre of the primary monitor
#   -Region left  : 60x600 at the left middle of the primary monitor
#   -Region tray  : 500x80 at the bottom-right of the primary monitor
#   -Region panel : 900x900 at the top centre of the primary monitor
param([ValidateSet("top", "left", "tray", "panel")][string]$Region = "top", [string]$Out = "$env:TEMP\claudehud-shot.png")
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -Namespace Native -Name Dpi -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(System.IntPtr v);'
[Native.Dpi]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null   # PER_MONITOR_AWARE_V2
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
switch ($Region) {
    "top"   { $r = [System.Drawing.Rectangle]::new($b.X + $b.Width / 2 - 300, $b.Y, 600, 60) }
    "left"  { $r = [System.Drawing.Rectangle]::new($b.X, $b.Y + $b.Height / 2 - 300, 60, 600) }
    "tray"  { $r = [System.Drawing.Rectangle]::new($b.Right - 500, $b.Bottom - 80, 500, 80) }
    "panel" { $r = [System.Drawing.Rectangle]::new($b.X + $b.Width / 2 - 450, $b.Y, 900, 900) }
}
$bmp = [System.Drawing.Bitmap]::new($r.Width, $r.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$op = [System.Drawing.CopyPixelOperation]::SourceCopy -bor [System.Drawing.CopyPixelOperation]::CaptureBlt
$g.CopyFromScreen($r.X, $r.Y, 0, 0, $r.Size, $op)
$bmp.Save($Out)
"saved $Out ($r)"
```

- [x] **Step 8: Verify visually**

```powershell
cargo build --release
$env:CLAUDEHUD_FIXTURE = (Resolve-Path fixtures\snapshots\yellow_waiting_beats_amber.json).Path
Start-Process target\release\claudehud.exe
Start-Sleep 2
powershell -ExecutionPolicy Bypass -File scripts\screenshot.ps1 -Region top
```

View `%TEMP%\claudehud-shot.png` with the Read tool. Expected: a yellow (`#E9DA4C`) 132 × 4 logical px pill centred at the very top. At this laptop's ~164% scaling that is about 216 × 7 physical px.

Then run `scripts\screenshot.ps1 -Region tray` and view it. Expected: the orange pixel creature with a yellow badge in the tray. If it is hidden in the overflow chevron, ask the user to drag it onto the taskbar once.

Also check:
- Ask the user to hover the tray icon. Expected tooltip: `portal-service: approve the permission prompt` / `2 running · 7d 92%`.
- Edit the fixture file, change `"waiting"` to `"idle"` and save. Within 1 s the strip turns green (the other session is busy).
- Start a second `claudehud.exe`. It exits at once: a single instance.
- Right-click the tray icon → Exit. The strip and the tray icon both disappear.
- Clear the fixture (`Remove-Item Env:CLAUDEHUD_FIXTURE`) and start again. There is no strip (off) and the tray shows the grey creature with no badge. Exit.

- [x] **Step 9: Lint and commit**

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/main.rs src/platform scripts/screenshot.ps1
git commit -m "feat(ui): strip window, tray icon and message loop driven by fixtures"
```
