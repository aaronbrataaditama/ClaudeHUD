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
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
    GetCursorPos, GetMessageW, PostMessageW, PostQuitMessage, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, MA_NOACTIVATE, MF_STRING, MSG,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_DESTROY, WM_MOUSEACTIVATE, WM_NULL,
    WM_RBUTTONUP, WM_TIMER,
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
    let Some(_instance) = system::single_instance() else {
        return;
    };
    let settings_path =
        settings::settings_path(&system::exe_dir(), system::appdata_dir().as_deref());
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
                    Snapshot {
                        now_ms: now,
                        ..Snapshot::default()
                    }
                }
            },
            None => Snapshot {
                now_ms: now,
                warn_percent: self.settings.warn_percent,
                ..Snapshot::default()
            },
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
        let badge = (!off).then_some(Badge {
            rgb: self.light.colour.rgb(),
            alpha,
        });
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
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                pt.x,
                pt.y,
                Some(0),
                self.controller,
                None,
            );
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
