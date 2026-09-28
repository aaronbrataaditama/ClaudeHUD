//! Message loop and wiring. Task 18 version: adds the hover panel.

use super::layered;
use super::localtime::local_parts;
use super::monitors;
use super::process::WinProbe;
use super::render::Renderer;
use super::system;
use super::tray::{Tray, WM_TRAY};
use super::win;
use super::worker::{Worker, WorkerMsg, WM_WORKER};
use crate::collect::Collector;
use crate::geometry::{self, MonitorInfo, Rect};
use crate::hover::{Action, Event, Hover};
use crate::icon::{self, Badge};
use crate::model::{Colour, Light, Snapshot, DIM_ALPHA};
use crate::panel::layout::{is_expanded, layout, Ctx, Hit, Layout, ViewState};
use crate::settings::{self, Settings};
use crate::{fixture, log, state, timefmt, tooltip};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{WM_MOUSEHOVER, WM_MOUSELEAVE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_HOVER, TME_LEAVE, TRACKMOUSEEVENT, TRACKMOUSEEVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
    GetCursorPos, GetMessageW, KillTimer, PostMessageW, PostQuitMessage, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, MA_NOACTIVATE, MF_STRING, MSG,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_DESTROY, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NULL, WM_RBUTTONUP, WM_TIMER,
};

const TIMER_TICK: usize = 1;
const TIMER_CLOSE: usize = 2;
const TIMER_ANIM: usize = 3;
const CMD_EXIT: u32 = 199;
const OPEN_MS: f32 = 180.0;
const CLOSE_MS: f32 = 140.0;
const HOVER_MS: u32 = 250;
const CLOSE_DELAY_MS: u32 = 300;
const STALE_USAGE_MS: i64 = 60_000;
const WHEEL_STEP: f32 = 44.0;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

#[derive(Clone, Copy)]
struct Anim {
    start: Instant,
    opening: bool,
}

struct App {
    controller: HWND,
    strip: HWND,
    panel: HWND,
    tray: Tray,
    taskbar_created: u32,
    settings: Settings,
    monitors: Vec<MonitorInfo>,
    fixture: Option<PathBuf>,
    collector: Collector,
    probe: WinProbe,
    worker: Option<Worker>,
    snapshot: Snapshot,
    light: Light,
    shown_strip: Option<(Rect, Colour, bool)>,
    renderer: Option<Renderer>,
    hover: Hover,
    view: ViewState,
    layout: Option<Layout>,
    panel_rect: Rect,
    panel_scale: f32,
    anim: Option<Anim>,
    strip_tracking: bool,
    panel_tracking: bool,
}

fn track(hwnd: HWND, flags: TRACKMOUSEEVENT_FLAGS, hover_ms: u32) {
    let mut t = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: flags,
        hwndTrack: hwnd,
        dwHoverTime: hover_ms,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut t);
    }
}

fn lparam_xy(lp: LPARAM) -> (i32, i32) {
    (
        (lp.0 & 0xFFFF) as u16 as i16 as i32,
        ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32,
    )
}

pub fn run() {
    let Some(_instance) = system::single_instance() else {
        return;
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
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
        || !win::register_class(hinst, w!("ClaudeHUDPanel"), Some(panel_proc))
    {
        log::warn("RegisterClassExW failed");
        return;
    }
    let (Ok(controller), Ok(strip), Ok(panel)) = (
        win::create_controller(hinst, w!("ClaudeHUDController")),
        win::create_layered(hinst, w!("ClaudeHUDStrip")),
        win::create_layered(hinst, w!("ClaudeHUDPanel")),
    ) else {
        log::warn("CreateWindowExW failed");
        return;
    };
    let renderer = match Renderer::new(hinst) {
        Ok(r) => Some(r),
        Err(e) => {
            log::warn(&format!("panel disabled: {e}"));
            None
        }
    };
    let fixture = std::env::var_os("CLAUDEHUD_FIXTURE").map(PathBuf::from);
    let claude_dir = system::claude_dir();
    let worker = fixture.is_none().then(|| {
        Worker::spawn(
            controller,
            claude_dir.clone(),
            settings.usage_poll_s as u64,
            settings.status_poll_s as u64,
        )
    });
    let app = App {
        controller,
        strip,
        panel,
        tray: Tray::new(controller),
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        settings,
        monitors: monitors::enumerate(),
        fixture,
        collector: Collector::new(claude_dir),
        probe: WinProbe::new(),
        worker,
        snapshot: Snapshot::default(),
        light: Light::off(),
        shown_strip: None,
        renderer,
        hover: Hover::default(),
        view: ViewState::default(),
        layout: None,
        panel_rect: Rect::default(),
        panel_scale: 1.0,
        anim: None,
        strip_tracking: false,
        panel_tracking: false,
    };
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    with_app(App::tick);
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
    // ------------------------------------------------------------ data

    fn tick(&mut self) {
        if self.fixture.is_none() {
            self.collector
                .tick(timefmt::now_ms(), &self.probe, self.hover.visible);
        }
        self.refresh();
    }

    fn drain_worker(&mut self) {
        let now = timefmt::now_ms();
        if let Some(w) = &self.worker {
            while let Ok(m) = w.results.try_recv() {
                match m {
                    WorkerMsg::Usage(o) => self.collector.apply_usage(o, now),
                    WorkerMsg::Status(r) => self.collector.apply_status(r),
                }
            }
        }
    }

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
            None => self.collector.snapshot(now, self.settings.warn_percent),
        };
        self.light = state::fold(&self.snapshot, Some(&self.light));
        self.update_strip();
        self.update_tray();
        if self.hover.visible {
            self.render_panel();
        }
    }

    fn monitor(&self) -> Option<MonitorInfo> {
        geometry::pick_monitor(&self.monitors, &self.settings.monitor).cloned()
    }

    fn update_strip(&mut self) {
        let Some(m) = self.monitor() else { return };
        self.hover.reveal_suppressed =
            geometry::edge_borders_other_monitor(&m, &self.monitors, self.settings.edge);
        if self.light.colour == Colour::Off {
            if self.shown_strip.take().is_some() {
                win::hide(self.strip);
            }
            return;
        }
        let r = geometry::strip_rect(&m, self.settings.edge);
        let key = (r, self.light.colour, self.light.dim);
        if self.shown_strip == Some(key) {
            win::set_topmost(self.strip);
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

    // ------------------------------------------------------------ panel

    /// Position and opacity for the current animation frame.
    fn frame(&self) -> (i32, i32, u8, bool) {
        let r = self.panel_rect;
        let Some(a) = self.anim else {
            return (r.x, r.y, 255, true);
        };
        let dur = if a.opening { OPEN_MS } else { CLOSE_MS };
        let t = (a.start.elapsed().as_secs_f32() * 1000.0 / dur).clamp(0.0, 1.0);
        let p = if a.opening {
            1.0 - (1.0 - t).powi(3)
        } else {
            1.0 - t
        };
        let (dx, dy) = geometry::slide_offset(self.settings.edge, p, self.panel_scale);
        (r.x + dx, r.y + dy, (p * 255.0).round() as u8, t >= 1.0)
    }

    /// Lays out and draws the panel, then shows it at the current frame.
    fn render_panel(&mut self) -> bool {
        let Some(m) = self.monitor() else {
            return false;
        };
        let Some(r) = self.renderer.as_ref() else {
            return false;
        };
        let strip = geometry::strip_rect(&m, self.settings.edge);
        let lay = {
            let measure = r.measure();
            let ctx = Ctx {
                snap: &self.snapshot,
                light: &self.light,
                view: &self.view,
                max_h: geometry::max_content_h(&m),
                measure: &measure,
                local: &local_parts,
            };
            layout(&ctx)
        };
        self.view.scroll = self.view.scroll.clamp(0.0, lay.scroll_max);
        self.panel_rect = geometry::panel_rect(&m, self.settings.edge, strip, lay.height);
        self.panel_scale = m.scale;
        let (x, y, alpha, _) = self.frame();
        let at = Rect {
            x,
            y,
            ..self.panel_rect
        };
        let panel = self.panel;
        let Some(r) = self.renderer.as_mut() else {
            return false;
        };
        if let Err(e) = r.render(&lay, self.panel_rect, m.scale) {
            log::warn(&e);
            return false;
        }
        r.present(panel, at, alpha);
        self.layout = Some(lay);
        true
    }

    fn show_panel(&mut self) {
        let now = timefmt::now_ms();
        if self.fixture.is_none() {
            self.collector.tick(now, &self.probe, true); // pull sub-agents right away
            self.snapshot = self.collector.snapshot(now, self.settings.warn_percent);
            if self
                .collector
                .usage_age_ms(now)
                .is_none_or(|age| age > STALE_USAGE_MS)
            {
                if let Some(w) = &self.worker {
                    w.refresh();
                }
            }
        }
        self.anim = Some(Anim {
            start: Instant::now(),
            opening: true,
        });
        if !self.render_panel() {
            self.anim = None;
            self.hover.visible = false;
            return;
        }
        win::show_noactivate(self.panel);
        unsafe {
            SetTimer(Some(self.controller), TIMER_ANIM, 16, None);
        }
    }

    fn hide_panel(&mut self) {
        self.view.hovered = None;
        self.anim = Some(Anim {
            start: Instant::now(),
            opening: false,
        });
        unsafe {
            SetTimer(Some(self.controller), TIMER_ANIM, 16, None);
        }
    }

    fn step_anim(&mut self) {
        let Some(a) = self.anim else {
            unsafe {
                let _ = KillTimer(Some(self.controller), TIMER_ANIM);
            }
            return;
        };
        let (x, y, alpha, done) = self.frame();
        layered::move_and_fade(self.panel, x, y, alpha);
        if done {
            self.anim = None;
            unsafe {
                let _ = KillTimer(Some(self.controller), TIMER_ANIM);
            }
            if !a.opening {
                win::hide(self.panel);
                self.layout = None;
                self.view.scroll = 0.0;
            }
        }
    }

    fn dispatch(&mut self, ev: Event) {
        for action in self.hover.step(ev) {
            match action {
                Action::Show => self.show_panel(),
                Action::Hide => self.hide_panel(),
                Action::StartCloseTimer => unsafe {
                    SetTimer(Some(self.controller), TIMER_CLOSE, CLOSE_DELAY_MS, None);
                },
                Action::CancelCloseTimer => unsafe {
                    let _ = KillTimer(Some(self.controller), TIMER_CLOSE);
                },
                Action::Acknowledge => {
                    self.collector.acknowledge();
                    self.refresh();
                }
                Action::PinChanged(p) => {
                    self.view.pinned = p;
                    if self.hover.visible {
                        self.render_panel();
                    }
                }
            }
        }
    }

    fn hit(&self, lp: LPARAM) -> Option<Hit> {
        let (px, py) = lparam_xy(lp);
        let s = self.panel_scale.max(0.1);
        let (x, y) = (
            px as f32 / s - geometry::SHADOW,
            py as f32 / s - geometry::SHADOW,
        );
        self.layout.as_ref().and_then(|l| l.hit_at(x, y).cloned())
    }

    // ------------------------------------------------------------ window procs

    fn on_strip(&mut self, msg: u32, _wp: WPARAM, _lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_MOUSEMOVE => {
                if !self.strip_tracking {
                    track(self.strip, TME_HOVER | TME_LEAVE, HOVER_MS);
                    self.strip_tracking = true;
                    self.dispatch(Event::StripEnter);
                }
            }
            WM_MOUSEHOVER => self.dispatch(Event::StripHover),
            WM_MOUSELEAVE => {
                self.strip_tracking = false;
                self.dispatch(Event::StripLeave);
            }
            WM_LBUTTONUP => self.dispatch(Event::StripClick),
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn on_panel(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_MOUSEMOVE => {
                if !self.panel_tracking {
                    track(self.panel, TME_LEAVE, 0);
                    self.panel_tracking = true;
                    self.dispatch(Event::PanelEnter);
                }
                let h = self.hit(lp);
                if h != self.view.hovered {
                    self.view.hovered = h;
                    self.render_panel();
                }
            }
            WM_MOUSELEAVE => {
                self.panel_tracking = false;
                if self.view.hovered.take().is_some() && self.hover.visible {
                    self.render_panel();
                }
                self.dispatch(Event::PanelLeave);
            }
            WM_LBUTTONUP => match self.hit(lp) {
                Some(Hit::Pin) => self.dispatch(Event::PinClick),
                Some(Hit::Session(id)) | Some(Hit::SessionName(id)) => {
                    let current = self
                        .snapshot
                        .sessions
                        .iter()
                        .find(|s| s.session_id == id)
                        .map(|s| is_expanded(&self.view, s));
                    if let Some(open) = current {
                        self.view.expanded.insert(id, !open);
                        self.render_panel();
                    }
                }
                Some(Hit::StatusLink) => system::open_url("https://status.claude.com"),
                None => {}
            },
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as u16 as i16;
                let max = self.layout.as_ref().map_or(0.0, |l| l.scroll_max);
                self.view.scroll =
                    (self.view.scroll - delta as f32 / 120.0 * WHEEL_STEP).clamp(0.0, max);
                self.render_panel();
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn on_controller(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_TIMER if wp.0 == TIMER_TICK => self.tick(),
            WM_TIMER if wp.0 == TIMER_CLOSE => {
                unsafe {
                    let _ = KillTimer(Some(self.controller), TIMER_CLOSE);
                }
                self.dispatch(Event::CloseTimer);
            }
            WM_TIMER if wp.0 == TIMER_ANIM => self.step_anim(),
            WM_WORKER => {
                self.drain_worker();
                self.refresh();
            }
            WM_TRAY => match (lp.0 as u32) & 0xFFFF {
                WM_LBUTTONUP => self.dispatch(Event::TrayClick),
                WM_RBUTTONUP => self.show_menu(),
                _ => {}
            },
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
    match with_app(|a| a.on_strip(msg, wp, lp)) {
        Some(Some(r)) => r,
        _ if msg == WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match with_app(|a| a.on_panel(msg, wp, lp)) {
        Some(Some(r)) => r,
        _ if msg == WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
