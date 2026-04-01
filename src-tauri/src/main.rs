// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    clipboard_diary_lib::run().expect("failed to launch Clipboard Diary");
}
