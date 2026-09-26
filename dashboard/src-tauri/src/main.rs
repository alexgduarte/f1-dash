// no console window next to the app on Windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    f1_dash_lib::run()
}
