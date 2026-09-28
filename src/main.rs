#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    claudehud::platform::app::run();
    #[cfg(not(windows))]
    eprintln!("ClaudeHUD runs on Windows only.");
}
