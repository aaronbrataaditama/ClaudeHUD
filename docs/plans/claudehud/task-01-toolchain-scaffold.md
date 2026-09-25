# Task 1: Toolchain and crate scaffold

**Goal:** A Rust toolchain is installed, the crate compiles as lib + bin with the release profile from the spec, resources (the manifest) are embedded, and `scripts/check.ps1` runs fmt, clippy, tests, a release build and the 2 MB size check.

**Files:**
- Create: `rust-toolchain.toml`, `.cargo/config.toml`, `Cargo.toml`, `build.rs`
- Create: `assets/claudehud.rc`, `assets/claudehud.manifest`
- Create: `src/main.rs`, `src/lib.rs`
- Create: `tests/common/mod.rs`, `tests/scaffold.rs`
- Create: `scripts/check.ps1`

**Interfaces:**
- Consumes: nothing.
- Produces: `tests/common/mod.rs` → `TempDir::new(tag) -> TempDir`, `TempDir::path(&self) -> &Path`, `TempDir::write(&self, rel: &str, contents: &str) -> PathBuf`, `TempDir::write_bytes(&self, rel: &str, bytes: &[u8]) -> PathBuf`. Every integration test file starts with `mod common;`.

---

- [x] **Step 1: Check for Rust; install it only with the user's permission**

Run: `cargo --version`
If it prints a version ≥ 1.82 (needed for `Option::is_none_or`), skip to Step 2. If it is older, run `rustup update stable`.
If `cargo` is not found, **stop and ask the user** for permission to install Rust (this changes their machine). With permission run:

```powershell
winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements
# open a new shell so PATH includes %USERPROFILE%\.cargo\bin, then:
rustup default stable-x86_64-pc-windows-msvc
rustup component add clippy rustfmt
cargo --version
```

Expected: `cargo 1.8x.x` (or newer). The MSVC Build Tools and Windows SDK 10.0.26100 are already installed on this machine (VS 18 BuildTools); if linking fails with `link.exe not found`, tell the user to add the "Desktop development with C++" workload.

- [x] **Step 2: Toolchain pin and static CRT**

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
targets = ["x86_64-pc-windows-msvc"]
```

`.cargo/config.toml`:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

- [x] **Step 3: `Cargo.toml`**

```toml
[package]
name = "claudehud"
version = "0.1.0"
edition = "2021"
description = "Claude Code status light for the Windows desktop"
build = "build.rs"
publish = false

[lib]
name = "claudehud"
path = "src/lib.rs"

[[bin]]
name = "claudehud"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[target.'cfg(windows)'.dependencies.windows]
version = "0.61"
features = [
  "Foundation_Numerics",
  "Win32_Foundation",
  "Win32_Security",
  "Win32_Graphics_Gdi",
  "Win32_Graphics_Direct2D",
  "Win32_Graphics_Direct2D_Common",
  "Win32_Graphics_DirectWrite",
  "Win32_Graphics_Dxgi_Common",
  "Win32_Graphics_Imaging",
  "Win32_UI_WindowsAndMessaging",
  "Win32_UI_Controls",
  "Win32_UI_Shell",
  "Win32_UI_HiDpi",
  "Win32_UI_Input_KeyboardAndMouse",
  "Win32_System_Com",
  "Win32_System_LibraryLoader",
  "Win32_System_Power",
  "Win32_System_Registry",
  "Win32_System_RemoteDesktop",
  "Win32_System_SystemInformation",
  "Win32_System_Threading",
  "Win32_System_Time",
  "Win32_Networking_WinHttp",
]

[build-dependencies]
embed-resource = "3"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

- [x] **Step 4: Resources (manifest now; the icon is added in Task 14)**

`assets/claudehud.manifest`:

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="ClaudeHUD" version="0.1.0.0"/>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
```

`assets/claudehud.rc` (resource type 24 = RT_MANIFEST, id 1 = the process manifest):

```
1 24 "claudehud.manifest"
```

`build.rs`:

```rust
fn main() {
    println!("cargo:rerun-if-changed=assets/claudehud.rc");
    println!("cargo:rerun-if-changed=assets/claudehud.manifest");
    println!("cargo:rerun-if-changed=assets/claudehud.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/claudehud.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile assets/claudehud.rc");
    }
}
```

If `embed_resource::NONE` or `.manifest_required()` do not exist in the resolved 3.x version, check `cargo doc -p embed-resource --open` and use the equivalent (older versions: `embed_resource::compile("assets/claudehud.rc", embed_resource::NONE);` with no result handling).

- [x] **Step 5: Entry points**

`src/lib.rs`:

```rust
//! ClaudeHUD: a Claude Code status light for the Windows desktop.
//!
//! Everything outside `platform` is plain Rust with no Windows dependency, so it
//! can be unit-tested on any machine. Modules are added task by task.
```

`src/main.rs` (Task 16 replaces the body):

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    println!("claudehud {}", env!("CARGO_PKG_VERSION"));
}
```

`windows_subsystem = "windows"` only applies to release builds, so `cargo run` (debug) keeps a console for `eprintln!` debugging.

- [x] **Step 6: Write the test helper and a failing test**

`tests/common/mod.rs`:

```rust
#![allow(dead_code)]
//! Shared helpers for integration tests. No external crates allowed, so this
//! replaces `tempfile`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("claudehud-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write(&self, rel: &str, contents: &str) -> PathBuf {
        self.write_bytes(rel, contents.as_bytes())
    }

    pub fn write_bytes(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).expect("create parent dir");
        }
        std::fs::write(&p, bytes).expect("write test file");
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
```

`tests/scaffold.rs`:

```rust
mod common;

#[test]
fn temp_dir_writes_nested_files_and_cleans_up() {
    let root;
    {
        let t = common::TempDir::new("scaffold");
        let p = t.write("a/b/c.txt", "hello");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
        root = t.path().to_path_buf();
        assert!(root.exists());
    }
    assert!(!root.exists(), "TempDir must delete itself on drop");
}
```

- [x] **Step 7: Run tests**

Run: `cargo test`
Expected: first build downloads crates and compiles `windows` (1–3 min), then `test temp_dir_writes_nested_files_and_cleans_up ... ok`.

- [x] **Step 8: The check script**

`scripts/check.ps1`:

```powershell
# Full local gate: formatting, lints, tests, release build, size budget.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Run($label, [scriptblock]$cmd) {
    Write-Host "== $label" -ForegroundColor Cyan
    & $cmd
    if ($LASTEXITCODE -ne 0) { Write-Host "FAILED: $label" -ForegroundColor Red; exit 1 }
}

Run "fmt"    { cargo fmt --check }
Run "clippy" { cargo clippy --all-targets -- -D warnings }
Run "test"   { cargo test }
Run "build"  { cargo build --release }

$exe = "target\release\claudehud.exe"
$size = (Get-Item $exe).Length
$budget = 2MB
if ($size -gt $budget) {
    Write-Host "FAILED: $exe is $size bytes, budget $budget" -ForegroundColor Red
    exit 1
}
Write-Host ("OK: {0} is {1:N0} KB (budget {2:N0} KB)" -f $exe, ($size / 1KB), ($budget / 1KB)) -ForegroundColor Green
```

Run: `powershell -ExecutionPolicy Bypass -File scripts/check.ps1`
Expected: all five sections pass; last line `OK: target\release\claudehud.exe is ~150 KB`.
If `cargo fmt --check` fails, run `cargo fmt` and re-run.

- [x] **Step 9: Verify the manifest is embedded**

Run: `target\release\claudehud.exe` from PowerShell.
Expected: nothing visible happens (release is a GUI-subsystem exe with no window yet) and it exits immediately. `cargo run` prints `claudehud 0.1.0`.

- [x] **Step 10: Commit** (the first commit also takes the existing docs)

```powershell
git add .gitignore CLAUDE.md .claude/settings.json .claude/PROGRESS.md PLAN-CLAUDEHUD.md claudehud-mockup.html ClaudeHUD_icon.jpg docs/plans
git add rust-toolchain.toml .cargo/config.toml Cargo.toml Cargo.lock build.rs assets src tests scripts/check.ps1
git status
git commit -m "chore: scaffold claudehud crate, manifest, check script"
```

Check `git status` before committing: nothing under `target/` and no `*.key` or credentials file may be staged.
