mod app;
mod platform;
mod storage;
mod types;

use app::ClipboardDiaryApp;
use eframe::{egui, NativeOptions};
use platform::load_window_icon;
use storage::append_log;

pub fn run() -> eframe::Result {
    let viewport = {
        let builder = egui::ViewportBuilder::default()
            .with_title("Vclipboard (All clips)")
            .with_inner_size([460.0, 680.0])
            .with_min_inner_size([400.0, 500.0]);

        match load_window_icon() {
            Ok(icon) => builder.with_icon(icon),
            Err(error) => {
                append_log(format!("window icon load failed: {error}"));
                builder
            }
        }
    };

    append_log("app run start");
    let native_options = NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Vclipboard",
        native_options,
        Box::new(|cc| Ok(Box::new(ClipboardDiaryApp::new(cc)))),
    )
}
