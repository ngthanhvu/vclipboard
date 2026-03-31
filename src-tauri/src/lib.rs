use chrono::{Local, TimeZone};
use directories::ProjectDirs;
use eframe::{
    egui::{
        self, Align, Button, Color32, Context, Frame, Layout, Margin, RichText, ScrollArea,
        Stroke, TextEdit, TopBottomPanel, Ui, Vec2,
    },
    App, CreationContext, NativeOptions,
};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_HISTORY_ITEMS: usize = 250;
const POLL_INTERVAL_MS: u64 = 900;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClipboardEntry {
    id: String,
    content: String,
    preview: String,
    created_at: u64,
    character_count: usize,
    line_count: usize,
}

struct HistoryStore {
    items: Mutex<Vec<ClipboardEntry>>,
    storage_path: PathBuf,
    monitor_started: AtomicBool,
}

impl HistoryStore {
    fn new(storage_path: PathBuf) -> Self {
        Self {
            items: Mutex::new(load_history(&storage_path)),
            storage_path,
            monitor_started: AtomicBool::new(false),
        }
    }

    fn history(&self) -> Vec<ClipboardEntry> {
        self.items
            .lock()
            .map(|items| items.clone())
            .unwrap_or_default()
    }

    fn start_monitor(self: &Arc<Self>, ctx: Context) {
        if self.monitor_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let store = Arc::clone(self);
        thread::spawn(move || {
            let mut last_clipboard = store
                .items
                .lock()
                .ok()
                .and_then(|items| items.first().map(|entry| entry.content.clone()))
                .unwrap_or_default();

            loop {
                if let Ok(content) = read_clipboard_text() {
                    if !content.trim().is_empty() && content != last_clipboard {
                        if store.record_content(content.clone()).is_some() {
                            last_clipboard = content;
                            ctx.request_repaint();
                        }
                    }
                }

                thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
        });
    }

    fn record_content(&self, content: String) -> Option<ClipboardEntry> {
        let entry = build_entry(content);
        let mut items = self.items.lock().ok()?;

        if items
            .first()
            .map(|current| current.content == entry.content)
            .unwrap_or(false)
        {
            return None;
        }

        items.retain(|item| item.content != entry.content);
        items.insert(0, entry.clone());
        if items.len() > MAX_HISTORY_ITEMS {
            items.truncate(MAX_HISTORY_ITEMS);
        }
        save_history(&self.storage_path, &items);
        Some(entry)
    }

    fn copy_entry(&self, id: &str) -> Result<(), String> {
        let items = self.history();
        let entry = items
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| String::from("Khong tim thay clip can copy"))?;

        write_clipboard_text(&entry.content)?;
        let _ = self.record_content(entry.content);
        Ok(())
    }

    fn delete_entry(&self, id: &str) -> Result<(), String> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| String::from("Khong the truy cap lich su clipboard"))?;
        let original_len = items.len();
        items.retain(|item| item.id != id);
        if items.len() == original_len {
            return Err(String::from("Khong tim thay muc can xoa"));
        }
        save_history(&self.storage_path, &items);
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| String::from("Khong the truy cap lich su clipboard"))?;
        items.clear();
        save_history(&self.storage_path, &items);
        Ok(())
    }
}

struct ClipboardDiaryApp {
    store: Arc<HistoryStore>,
    search: String,
    selected_id: Option<String>,
    status_message: String,
}

impl ClipboardDiaryApp {
    fn new(cc: &CreationContext<'_>) -> Self {
        configure_visuals(&cc.egui_ctx);

        let storage_path = app_storage_path();
        let store = Arc::new(HistoryStore::new(storage_path));
        store.start_monitor(cc.egui_ctx.clone());

        let selected_id = store.history().first().map(|entry| entry.id.clone());
        Self {
            store,
            search: String::new(),
            selected_id,
            status_message: String::from("Ready"),
        }
    }

    fn filtered_history(&self) -> Vec<ClipboardEntry> {
        let items = self.store.history();
        let keyword = self.search.trim().to_lowercase();
        if keyword.is_empty() {
            return items;
        }

        items.into_iter()
            .filter(|entry| {
                let haystack = format!("{}\n{}", entry.preview, entry.content).to_lowercase();
                haystack.contains(&keyword)
            })
            .collect()
    }

    fn selected_entry<'a>(&self, items: &'a [ClipboardEntry]) -> Option<&'a ClipboardEntry> {
        self.selected_id
            .as_ref()
            .and_then(|id| items.iter().find(|item| item.id == *id))
            .or_else(|| items.first())
    }

    fn ensure_selection(&mut self, items: &[ClipboardEntry]) {
        let exists = self
            .selected_id
            .as_ref()
            .map(|id| items.iter().any(|item| item.id == *id))
            .unwrap_or(false);

        if !exists {
            self.selected_id = items.first().map(|item| item.id.clone());
        }
    }

    fn copy_selected(&mut self, items: &[ClipboardEntry]) {
        if let Some(entry) = self.selected_entry(items) {
            match self.store.copy_entry(&entry.id) {
                Ok(()) => {
                    self.status_message =
                        format!("Copied '{}' back to clipboard", truncate(&entry.preview, 36));
                }
                Err(error) => self.status_message = error,
            }
        }
    }

    fn delete_selected(&mut self, items: &[ClipboardEntry]) {
        if let Some(entry) = self.selected_entry(items) {
            let entry_id = entry.id.clone();
            match self.store.delete_entry(&entry_id) {
                Ok(()) => {
                    self.status_message = String::from("Deleted selected clip");
                    self.selected_id = None;
                }
                Err(error) => self.status_message = error,
            }
        }
    }

    fn clear_history(&mut self) {
        match self.store.clear() {
            Ok(()) => {
                self.selected_id = None;
                self.status_message = String::from("Cleared clipboard history");
            }
            Err(error) => self.status_message = error,
        }
    }

    fn top_toolbar(&mut self, ctx: &Context) {
        TopBottomPanel::top("toolbar")
            .exact_height(38.0)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(236, 236, 236))
                    .inner_margin(Margin::symmetric(6, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.add(Button::new("Copy")).clicked() {
                        let items = self.filtered_history();
                        self.copy_selected(&items);
                    }
                    if ui.add(Button::new("Delete")).clicked() {
                        let items = self.filtered_history();
                        self.delete_selected(&items);
                    }
                    if ui.add(Button::new("Clear all")).clicked() {
                        self.clear_history();
                    }
                    ui.separator();
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("Clipboard history")
                            .strong()
                            .color(Color32::from_rgb(38, 38, 38)),
                    );
                    ui.add_space(8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let available = ui.available_width();
                        if available > 150.0 {
                            ui.label(
                                RichText::new("Double click a row to copy")
                                    .color(Color32::from_rgb(90, 90, 90)),
                            );
                        }
                    });
                });
            });
    }

    fn bottom_bar(&mut self, ctx: &Context, visible_count: usize, total_count: usize) {
        TopBottomPanel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(240, 240, 240))
                    .inner_margin(Margin::symmetric(8, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [140.0, 22.0],
                        TextEdit::singleline(&mut self.search).hint_text("Search clips..."),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("{visible_count}/{total_count} clips"))
                            .color(Color32::from_rgb(50, 50, 50)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(&self.status_message).color(Color32::from_rgb(55, 55, 55)),
                    );
                });
            });
    }

    fn history_ui(&mut self, ui: &mut Ui) {
        let items = self.filtered_history();
        let total_items = self.store.history().len();
        self.ensure_selection(&items);
        let selected_id = self.selected_entry(&items).map(|entry| entry.id.clone());
        let compact = ui.available_width() < 520.0;

        ui.vertical(|ui| {
            Frame::group(ui.style())
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    let mut table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(false)
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .column(Column::exact(24.0))
                        .column(Column::remainder());

                    if !compact {
                        table = table.column(Column::exact(92.0));
                    }

                    table.header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.label(RichText::new("#").strong().color(Color32::from_rgb(48, 48, 48)));
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("Clipboard history")
                                        .strong()
                                        .color(Color32::from_rgb(48, 48, 48)),
                                );
                            });
                            if !compact {
                                header.col(|ui| {
                                    ui.label(
                                        RichText::new("Captured")
                                            .strong()
                                            .color(Color32::from_rgb(48, 48, 48)),
                                    );
                                });
                            }
                        })
                        .body(|body| {
                            body.rows(22.0, items.len(), |mut row| {
                                let index = row.index();
                                let entry = &items[index];
                                let is_selected = selected_id
                                    .as_ref()
                                    .map(|current| current == &entry.id)
                                    .unwrap_or(false);

                                row.col(|ui| {
                                    let icon = if entry.line_count > 1 { "S" } else { "T" };
                                    let text = if is_selected {
                                        RichText::new(icon).strong().color(Color32::WHITE)
                                    } else {
                                        RichText::new(icon).strong().color(Color32::from_rgb(0, 70, 160))
                                    };
                                    ui.label(text);
                                });

                                row.col(|ui| {
                                    let preview_text = if compact {
                                        truncate(&entry.preview, 40)
                                    } else {
                                        truncate(&entry.preview, 86)
                                    };
                                    let response = ui
                                        .allocate_ui_with_layout(
                                            Vec2::new(ui.available_width(), 18.0),
                                            Layout::left_to_right(Align::Center),
                                            |ui| ui.selectable_label(is_selected, preview_text),
                                        )
                                        .inner
                                        .on_hover_text(&entry.preview);
                                    if response.clicked() {
                                        self.selected_id = Some(entry.id.clone());
                                    }
                                    if response.double_clicked() {
                                        match self.store.copy_entry(&entry.id) {
                                            Ok(()) => {
                                                self.status_message = format!(
                                                    "Copied '{}' back to clipboard",
                                                    truncate(&entry.preview, 36)
                                                );
                                            }
                                            Err(error) => self.status_message = error,
                                        }
                                    }
                                    response.context_menu(|ui| {
                                        if ui.button("Copy to clipboard").clicked() {
                                            let _ = self.store.copy_entry(&entry.id);
                                            self.status_message = String::from("Copied selected clip");
                                            ui.close();
                                        }
                                        if ui.button("Delete").clicked() {
                                            let _ = self.store.delete_entry(&entry.id);
                                            self.status_message = String::from("Deleted selected clip");
                                            self.selected_id = None;
                                            ui.close();
                                        }
                                    });
                                });

                                if !compact {
                                    row.col(|ui| {
                                        let color = if is_selected {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_gray(90)
                                        };
                                        ui.label(
                                            RichText::new(format_timestamp(entry.created_at)).color(color),
                                        );
                                    });
                                }
                            });
                        });
                });

            ui.add_space(6.0);
            Frame::group(ui.style())
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("Preview")
                                .strong()
                                .color(Color32::from_rgb(45, 45, 45)),
                        );
                        ui.separator();
                        if let Some(entry) = self.selected_entry(&items) {
                            ui.label(
                                RichText::new(format!(
                                    "{} chars | {} lines",
                                    entry.character_count, entry.line_count
                                ))
                                .color(Color32::from_rgb(60, 60, 60)),
                            );
                            ui.separator();
                            if ui.button("Copy").clicked() {
                                self.copy_selected(&items);
                            }
                            if ui.button("Delete").clicked() {
                                self.delete_selected(&items);
                            }
                        } else {
                            ui.label("No clip selected");
                        }
                    });
                    ui.separator();

                    let preview_height = (ui.available_height() - 6.0).max(120.0);
                    ScrollArea::vertical()
                        .max_height(preview_height)
                        .show(ui, |ui| {
                            if let Some(entry) = self.selected_entry(&items) {
                                let mut preview_text = entry.content.clone();
                                ui.add_sized(
                                    [ui.available_width(), preview_height],
                                    TextEdit::multiline(&mut preview_text)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .interactive(false),
                                );
                            } else if total_items == 0 {
                                ui.label("Clipboard history is empty. Copy any text in Windows to start.");
                            } else {
                                ui.label("No clip matches the current search.");
                            }
                        });
                });
        });
    }

}

impl App for ClipboardDiaryApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.top_toolbar(ctx);

        let visible_count = self.filtered_history().len();
        let total_count = self.store.history().len();
        self.bottom_bar(ctx, visible_count, total_count);

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(230, 230, 230))
                    .inner_margin(Margin::same(8)),
            )
            .show(ctx, |ui| self.history_ui(ui));
    }
}

fn configure_visuals(ctx: &Context) {
    let mut visuals = egui::Visuals::light();
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(236, 236, 236);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(68, 68, 68));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(248, 248, 248);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(44, 44, 44));
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(255, 255, 255);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(24, 24, 24));
    visuals.widgets.active.bg_fill = Color32::from_rgb(12, 104, 204);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.open.bg_fill = Color32::from_rgb(242, 242, 242);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, Color32::from_rgb(32, 32, 32));
    visuals.selection.bg_fill = Color32::from_rgb(12, 104, 204);
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.panel_fill = Color32::from_rgb(230, 230, 230);
    visuals.window_fill = Color32::from_rgb(236, 236, 236);
    visuals.window_stroke = Stroke::new(1.0, Color32::from_gray(160));
    visuals.override_text_color = Some(Color32::from_rgb(32, 32, 32));
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 4.0);
    style.spacing.button_padding = Vec2::new(8.0, 3.0);
    ctx.set_style(style);
}

fn build_entry(content: String) -> ClipboardEntry {
    let created_at = now_millis();
    let preview = build_preview(&content);
    let character_count = content.chars().count();
    let line_count = content.lines().count().max(1);

    ClipboardEntry {
        id: format!("{created_at}-{character_count}"),
        content,
        preview,
        created_at,
        character_count,
        line_count,
    }
}

fn build_preview(content: &str) -> String {
    let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview: String = collapsed.chars().take(120).collect();
    if collapsed.chars().count() > 120 {
        format!("{preview}...")
    } else {
        preview
    }
}

fn app_storage_path() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "ngthanhvu", "ClipboardDiary") {
        let base_dir = project_dirs.data_dir();
        let _ = fs::create_dir_all(base_dir);
        return base_dir.join("clipboard-history.json");
    }

    PathBuf::from("clipboard-history.json")
}

fn load_history(path: &PathBuf) -> Vec<ClipboardEntry> {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<Vec<ClipboardEntry>>(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_history(path: &PathBuf, items: &[ClipboardEntry]) {
    if let Ok(json) = serde_json::to_string_pretty(items) {
        let _ = fs::write(path, json);
    }
}

fn read_clipboard_text() -> Result<String, String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-Clipboard -Raw"])
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .replace("\r\n", "\n")
        .trim_end_matches('\n')
        .to_string())
}

fn write_clipboard_text(content: &str) -> Result<(), String> {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-Command", "$input | Set-Clipboard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(content.as_bytes())
            .map_err(|error| error.to_string())?;
    }

    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn format_timestamp(timestamp: u64) -> String {
    match Local.timestamp_millis_opt(timestamp as i64).single() {
        Some(date_time) => date_time.format("%H:%M:%S %d-%m").to_string(),
        None => String::from("--:--:--"),
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

pub fn run() -> eframe::Result {
    let native_options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Clipdiary (Clipboard history : All clips)")
            .with_inner_size([460.0, 680.0])
            .with_min_inner_size([400.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Clipboard Diary",
        native_options,
        Box::new(|cc| Ok(Box::new(ClipboardDiaryApp::new(cc)))),
    )
}
