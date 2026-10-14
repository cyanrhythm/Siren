//! Siren 可执行入口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    siren_app::run();
}
