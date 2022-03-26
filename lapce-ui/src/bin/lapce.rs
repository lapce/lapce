#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use lapce_ui::app;

pub fn main() {
    app::launch();
}
