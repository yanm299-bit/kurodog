#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|a| a == "--mcp") {
        kurodog_lib::run_mcp_stdio();
        return;
    }
    kurodog_lib::run();
}
