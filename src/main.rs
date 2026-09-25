#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    println!("claudehud {}", env!("CARGO_PKG_VERSION"));
}
