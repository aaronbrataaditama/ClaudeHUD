# Task 20: Smoke test, budgets, manual checklist

**Goal:** Lock in the things unit tests cannot see. An ignored-by-default Win32 smoke test asserts the strip's window styles, no focus theft and single-instance behaviour. A runtime budget script checks exe size and working set. A written manual checklist mirrors spec §12. Then a final full-suite run.

**Spec:** §8 items 5–6 (smoke test, budgets), §9 (targets: exe < 2 MB, working set < 40 MB), §12 (verification).

**Files:**
- Create: `tests/smoke.rs`
- Create: `scripts/budget.ps1`
- Create: `docs/manual-checklist.md`

**Interfaces:**
- Consumes: the built binary (`env!("CARGO_BIN_EXE_claudehud")`), `fixtures/snapshots/green_working.json`.
- Produces: nothing new for code; this is the release gate.

---

- [ ] **Step 1: Win32 smoke test**

`tests/smoke.rs`:

```rust
//! Launches the real exe and inspects its windows. Needs a desktop session, so it
//! is ignored by default. Run: `cargo test --test smoke -- --ignored --test-threads=1`
#![cfg(windows)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowLongPtrW, IsWindowVisible, GWL_EXSTYLE, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn launch() -> Running {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/snapshots/green_working.json");
    Running(Command::new(env!("CARGO_BIN_EXE_claudehud")).env("CLAUDEHUD_FIXTURE", fixture).spawn().expect("spawn claudehud"))
}

fn wait_for_strip() -> HWND {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(h) = unsafe { FindWindowW(w!("ClaudeHUDStrip"), None) } {
            if !h.is_invalid() && unsafe { IsWindowVisible(h) }.as_bool() {
                return h;
            }
        }
        assert!(Instant::now() < deadline, "strip window never became visible");
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
    assert_eq!(unsafe { GetForegroundWindow() }, before, "ClaudeHUD changed the foreground window");
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
        assert!(Instant::now() < deadline, "second instance is still running");
        std::thread::sleep(Duration::from_millis(100));
    }
}
```

Run: `cargo test --test smoke -- --ignored --test-threads=1`
Expected: 2 passed (the default `cargo test` shows them as ignored). Run it from a terminal that has focus. If `FindWindowW` returns `Result<HWND>` in the pinned version the code above already handles it; if it returns a bare `HWND`, drop the `Ok(..)` pattern.

- [ ] **Step 2: Runtime budget script**

`scripts/budget.ps1`:

```powershell
# Release gate for §9: exe size and working set after 60 s.
# Hover the strip once during the 60 s wait so the panel's Direct2D resources are counted.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build --release
if ($LASTEXITCODE -ne 0) { exit 1 }
$exe = Resolve-Path "target\release\claudehud.exe"
$sizeKb = [math]::Round((Get-Item $exe).Length / 1KB)
Get-Process claudehud -ErrorAction SilentlyContinue | Stop-Process
$p = Start-Process $exe -PassThru
Write-Host "Running. Hover the strip once to open the panel. Measuring in 60 s..."
Start-Sleep 60
$p.Refresh()
$wsMb = [math]::Round($p.WorkingSet64 / 1MB, 1)
$cpu = [math]::Round($p.TotalProcessorTime.TotalSeconds, 2)
Stop-Process $p
"exe: $sizeKb KB (budget 2048 KB)"
"working set: $wsMb MB (budget 40 MB)"
"CPU used in 60 s: $cpu s (expect well under 1 s)"
if ($sizeKb -gt 2048 -or $wsMb -gt 40) { Write-Host "OVER BUDGET" -ForegroundColor Red; exit 1 }
Write-Host "Within budget" -ForegroundColor Green
```

Run: `powershell -ExecutionPolicy Bypass -File scripts/budget.ps1` and hover the strip once while it waits (or ask the user to).
Expected: `Within budget`. If the working set is over 40 MB, check that `Renderer` is created once (not per render) and that `Surface` is reused while its size is unchanged, before questioning the budget.

- [ ] **Step 3: Manual checklist**

`docs/manual-checklist.md`:

```markdown
# ClaudeHUD manual release checklist (spec §12)

Run from `target\release\claudehud.exe` unless noted. Tick every line before a release.

## Light
- [ ] Two sessions open: strip green while one works, dim green when both are idle, hidden when both close.
- [ ] Permission prompt in a `--permission-mode default` session: yellow within ~1 s; green again after answering.
- [ ] `AskUserQuestion` and plan approval: yellow; clears on answer.
- [ ] 15-minute build in a session: stays green the whole time. (The most important regression check.)
- [ ] `CLAUDEHUD_FIXTURE=fixtures\snapshots\amber_quota.json`: amber. `red_quota_spent.json`: red. `off_no_sessions.json`: off.
- [ ] Kill a busy session's terminal: red; tooltip says "crashed mid-turn"; open and close the panel → no longer red.
- [ ] Restart ClaudeHUD with a stale busy registry file present: no red.

## Panel
- [ ] Hover the strip for ¼ s: the panel slides out from the strip. Sweeping across the strip opens nothing.
- [ ] Leave: closes ~⅓ s later; moving strip → panel keeps it open.
- [ ] Pin via the glyph and via a strip click; unpin; tray left-click toggles.
- [ ] Session name hover shows the working-folder tooltip, never clipped.
- [ ] Sub-agents appear under the session that spawned them; running → done.
- [ ] Mouse wheel scrolls the list when there are many sessions; header, usage and footer stay put.
- [ ] Plan label is right ("Team · Max 5x" here); Enterprise fixture shows "$50 of $600 spent".
- [ ] Clicking inside the panel never takes focus from the editor.
- [ ] Status link opens status.claude.com.

## Placement and system
- [ ] Edge Top/Left; Monitor switching across the mixed-DPI pair (strip is 132×4 logical px on both).
- [ ] Taskbar moved or resized: strip re-centres on the work area.
- [ ] Fullscreen app: strip hidden, returns afterwards.
- [ ] Win+L then unlock: nothing over the lock screen; strip back after unlock.
- [ ] Sleep and resume: correct state within 2 s.
- [ ] Explorer restart: the tray icon returns.
- [ ] Run on startup writes/removes `HKCU\…\Run\ClaudeHUD`; moving the exe re-points it on next launch.
- [ ] Second copy of the exe exits immediately.

## Portability and budgets
- [ ] Copy only `claudehud.exe` to a new folder on another Windows 11 machine: runs with no installer.
- [ ] Nothing is written under `%USERPROFILE%\.claude` (compare folder timestamps before and after a run).
- [ ] `scripts\budget.ps1`: exe < 2 MB, working set < 40 MB, CPU well under 1 s per minute.
- [ ] `cargo test --test smoke -- --ignored --test-threads=1` passes.
```

- [ ] **Step 4: Final gate**

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check.ps1
cargo test --test smoke -- --ignored --test-threads=1
powershell -ExecutionPolicy Bypass -File scripts/budget.ps1
```

Expected: all pass. Then walk through `docs/manual-checklist.md` with the user and tick what they confirm.

- [ ] **Step 5: Commit**

```powershell
git add tests/smoke.rs scripts/budget.ps1 docs/manual-checklist.md
git commit -m "test: Win32 smoke test, runtime budget script, manual release checklist"
```
