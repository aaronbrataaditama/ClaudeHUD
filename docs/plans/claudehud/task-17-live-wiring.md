# Task 17: Live data → strip and tray

**Goal:** Replace fixture-only mode with real data. The UI thread ticks the `Collector` every second (registry, transcripts, crash latch). A worker thread runs `fetch_usage` / `fetch_status` on the `Schedule` over WinHTTP and posts results back. `CLAUDEHUD_FIXTURE` still overrides everything and starts no worker.

**Spec:** §2 (one UI thread plus one worker thread, `PostMessage`), §3.7 (1 s tick), §3.5–3.6 (poll intervals), §7.

**Files:**
- Create: `src/platform/worker.rs`
- Replace: `src/platform/app.rs` (full new version below)
- Modify: `src/platform/mod.rs` (add `pub mod worker;`)

**Interfaces:**
- Consumes: `collect::{Collector, Schedule, fetch_usage, fetch_status, UsageOutcome, USER_AGENT}`, `platform::{http::WinHttp, process::WinProbe, system::claude_dir}`, everything Task 16 used.
- Produces:
  - `platform::worker::{WM_WORKER, Worker, WorkerMsg}`; `Worker::{spawn(notify: HWND, claude_dir: PathBuf, usage_s: u64, status_s: u64) -> Worker, refresh(&self), set_polls(&self, u64, u64)}`; `Worker.results: Receiver<WorkerMsg>`; `WorkerMsg::{Usage(UsageOutcome), Status(Result<ServiceStatus, String>)}`
  - `app.rs` gains `tick()` (collect then refresh) and `drain_worker()`; Tasks 18–19 build on these names.

---

- [x] **Step 1: Worker thread**

`src/platform/worker.rs`:

```rust
//! Polls usage and service status on the Schedule, off the UI thread (§2).
//! Results go through a channel; a posted WM_WORKER wakes the UI thread.

use super::http::WinHttp;
use crate::collect::{fetch_status, fetch_usage, Schedule, UsageOutcome, USER_AGENT};
use crate::model::ServiceStatus;
use crate::{log, timefmt};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_WORKER: u32 = WM_APP + 2;

pub enum WorkerMsg {
    Usage(UsageOutcome),
    Status(Result<ServiceStatus, String>),
}

enum WorkerCmd {
    Refresh,
    Polls { usage_s: u64, status_s: u64 },
}

pub struct Worker {
    cmds: Sender<WorkerCmd>,
    pub results: Receiver<WorkerMsg>,
}

impl Worker {
    pub fn spawn(notify: HWND, claude_dir: PathBuf, usage_s: u64, status_s: u64) -> Worker {
        let (cmd_tx, cmd_rx) = channel();
        let (res_tx, res_rx) = channel();
        // HWND is not Send; pass the raw value and rebuild it on the worker.
        let target = notify.0 as isize;
        let spawned = std::thread::Builder::new()
            .name("claudehud-worker".to_string())
            .spawn(move || worker_loop(target, claude_dir, usage_s, status_s, cmd_rx, res_tx));
        if let Err(e) = spawned {
            log::warn(&format!("could not start worker thread: {e}"));
        }
        Worker { cmds: cmd_tx, results: res_rx }
    }

    /// Resume, unlock, or panel opened with stale data. Respects rate-limit backoff.
    pub fn refresh(&self) {
        let _ = self.cmds.send(WorkerCmd::Refresh);
    }

    pub fn set_polls(&self, usage_s: u64, status_s: u64) {
        let _ = self.cmds.send(WorkerCmd::Polls { usage_s, status_s });
    }
}

fn post(target: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(target as *mut c_void)), WM_WORKER, WPARAM(0), LPARAM(0));
    }
}

fn worker_loop(target: isize, claude_dir: PathBuf, mut usage_s: u64, mut status_s: u64, cmds: Receiver<WorkerCmd>, out: Sender<WorkerMsg>) {
    let http = match WinHttp::new(USER_AGENT) {
        Ok(h) => h,
        Err(e) => {
            log::warn(&format!("worker: {e}"));
            return;
        }
    };
    let mut sched = Schedule::new(timefmt::now_ms());
    loop {
        let now = timefmt::now_ms();
        if sched.usage_due(now) {
            let o = fetch_usage(&claude_dir, &http, now);
            sched.after_usage(now, &o.result, usage_s);
            if out.send(WorkerMsg::Usage(o)).is_err() {
                return;
            }
            post(target);
        }
        let now = timefmt::now_ms();
        if sched.status_due(now) {
            let r = fetch_status(&http, now);
            sched.after_status(now, r.is_ok(), status_s);
            if out.send(WorkerMsg::Status(r)).is_err() {
                return;
            }
            post(target);
        }
        let wait_ms = (sched.next_wake_ms() - timefmt::now_ms()).clamp(250, 3_600_000) as u64;
        match cmds.recv_timeout(Duration::from_millis(wait_ms)) {
            Ok(WorkerCmd::Refresh) => sched.refresh_now(timefmt::now_ms()),
            Ok(WorkerCmd::Polls { usage_s: u, status_s: s }) => {
                usage_s = u;
                status_s = s;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
```

Add `pub mod worker;` to `src/platform/mod.rs`.

- [x] **Step 2: Replace `src/platform/app.rs`**

Full new content. Changes from Task 16 are `collector`, `probe` and `worker` fields, `tick()`, `drain_worker()` and `WM_WORKER`.

```rust
//! Message loop and wiring. Task 17 version: live data into the strip and tray.

use super::layered;
use super::localtime::local_parts;
use super::monitors;
use super::process::WinProbe;
use super::system;
use super::tray::{Tray, WM_TRAY};
use super::win;
use super::worker::{Worker, WorkerMsg, WM_WORKER};
use crate::collect::Collector;
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
    collector: Collector,
    probe: WinProbe,
    worker: Option<Worker>,
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
    let fixture = std::env::var_os("CLAUDEHUD_FIXTURE").map(PathBuf::from);
    let claude_dir = system::claude_dir();
    // Fixture mode replaces every collector, including the network ones.
    let worker = fixture.is_none().then(|| {
        Worker::spawn(controller, claude_dir.clone(), settings.usage_poll_s as u64, settings.status_poll_s as u64)
    });
    let app = App {
        controller,
        strip,
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
    /// 1 s tick: collect, then redraw what changed.
    fn tick(&mut self) {
        if self.fixture.is_none() {
            self.collector.tick(timefmt::now_ms(), &self.probe, false);
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
                    Snapshot { now_ms: now, ..Snapshot::default() }
                }
            },
            None => self.collector.snapshot(now, self.settings.warn_percent),
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
        let badge = (!off).then_some(Badge { rgb: self.light.colour.rgb(), alpha });
        let px = icon::render_tray_icon(size, badge, off);
        let tip = tooltip::tooltip(&self.light, &self.snapshot, &local_parts);
        self.tray.update(size, &px, &tip);
    }

    fn on_controller(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_TIMER if wp.0 == TIMER_TICK => self.tick(),
            WM_WORKER => {
                self.drain_worker();
                self.refresh();
            }
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

- [x] **Step 3: Build**

Run: `cargo build --release`, `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [x] **Step 4: Verify against real sessions**

Note (2026-09-28): only the first, safe check below was run by the implementing sub-agent — it
started `claudehud.exe` with `CLAUDEHUD_FIXTURE` unset (this session is itself a live, busy Claude
Code session) and confirmed a green strip. Checks 1-6 that touch other live sessions, the user's
Wi-Fi, or require an interactive second terminal were deliberately **not** attempted by the
sub-agent per the coordinator's scope limit, and are deferred to the coordinator/user to run
together:

The implementer is itself a Claude Code session, which makes a live test easy:

```powershell
Remove-Item Env:CLAUDEHUD_FIXTURE -ErrorAction SilentlyContinue
Start-Process target\release\claudehud.exe
Start-Sleep 3
powershell -ExecutionPolicy Bypass -File scripts\screenshot.ps1 -Region top
```

Expected: a **green** strip. This session is busy while it runs the command. View the screenshot to confirm. Then ask the user:

1. Hover the tray icon. Expected line one `<this session's name> working` (or `… and N more working`). Line two within ~5 s of start: `N running · 5h X% · 7d Y%`, where the percentages match `/usage` in Claude Code.
2. Stop typing to Claude and wait until this session is idle. Expected: the strip dims (green at 55%).
3. In another terminal run `claude --permission-mode default` and ask it to run `dir`. When the permission prompt appears the strip turns **yellow** within ~1 s; after answering it goes back to green or dim.
4. Close every Claude Code window. The strip disappears (off) and the tray creature turns grey. Line two still shows usage.
5. Kill a busy session's terminal window mid-turn (the Task 2 spike step 7 again). Expected: the strip turns **red** and the tooltip says `… crashed mid-turn · click to acknowledge`. There is no panel yet to acknowledge it, so Exit ClaudeHUD from the tray and restart it: after the restart the stale file must **not** show red (Review Focus #2).
6. Turn Wi-Fi off for 10 s. Nothing changes colour; the tooltip's line two may show `usage unavailable · offline` at the next poll.

- [x] **Step 5: Commit**

```powershell
cargo fmt
git add src/platform
git commit -m "feat(ui): live collector tick and worker thread for usage/status"
```
