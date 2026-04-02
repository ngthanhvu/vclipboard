use eframe::{
    egui::{
        self, Align, Align2, Button, Color32, Context, Frame, Key, KeyboardShortcut, Layout,
        Margin, Modifiers, RichText, Sense, Stroke, TextEdit, TopBottomPanel, Ui, Vec2,
        ViewportCommand,
    },
    App, CreationContext,
};
use egui_extras::{Column, TableBuilder};
use global_hotkey::{hotkey::HotKey, GlobalHotKeyManager};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};

use crate::{
    platform::{
        apply_window_visibility, capture_native_window_handle, create_tray,
        parse_hotkey_setting, record_hotkey_from_input, spawn_external_event_forwarders,
    },
    storage::{
        app_storage_path, append_log, format_timestamp, load_settings, save_settings,
        settings_path, truncate,
    },
    types::{AppSettings, ClipboardEntry, ClipboardEntryKind, HistoryStore, RuntimeShared, TrayHandles},
};

pub(crate) struct ClipboardDiaryApp {
    store: Arc<HistoryStore>,
    search: String,
    selected_id: Option<String>,
    status_message: String,
    settings: AppSettings,
    show_settings: bool,
    hotkey_manager: Option<GlobalHotKeyManager>,
    registered_hotkey: Option<HotKey>,
    tray: Option<TrayHandles>,
    tray_error: Option<String>,
    window_visible: bool,
    quit_requested: bool,
    startup_hide_pending: bool,
    start_hidden_requested: bool,
    start_hidden_effective: bool,
    settings_hotkey_input: String,
    settings_start_hidden: bool,
    capturing_hotkey: bool,
    search_has_focus: bool,
    runtime_shared: Arc<RuntimeShared>,
}

impl ClipboardDiaryApp {
    pub(crate) fn new(cc: &CreationContext<'_>) -> Self {
        configure_visuals(&cc.egui_ctx);

        let storage_path = app_storage_path();
        let store = Arc::new(HistoryStore::new(storage_path));
        store.start_monitor(cc.egui_ctx.clone());
        let settings = load_settings(&settings_path());
        let hotkey_manager = GlobalHotKeyManager::new().ok();
        let (tray, tray_error) = match create_tray() {
            Ok(tray) => (Some(tray), None),
            Err(error) => (None, Some(error)),
        };
        let settings_hotkey_input = settings.hotkey.clone();
        let settings_start_hidden = settings.start_hidden_in_tray;
        let startup_hidden = settings.start_hidden_in_tray;
        let tray_available = tray.is_some();
        let startup_hide_pending = false;
        let runtime_shared = Arc::new(RuntimeShared {
            window_visible: AtomicBool::new(true),
            native_hwnd: std::sync::atomic::AtomicIsize::new(
                capture_native_window_handle(cc).unwrap_or(0),
            ),
            hotkey_id: AtomicU32::new(0),
            open_settings: AtomicBool::new(false),
            last_hotkey_toggle: Mutex::new(None),
            last_tray_toggle: Mutex::new(None),
        });
        if let Some(tray_handles) = tray.as_ref() {
            spawn_external_event_forwarders(
                cc.egui_ctx.clone(),
                Arc::clone(&runtime_shared),
                tray_handles.show_hide_id.clone(),
                tray_handles.settings_id.clone(),
                tray_handles.quit_id.clone(),
            );
        }
        let initial_status = if let Some(error) = tray_error.as_ref() {
            format!("Tray icon unavailable, starting visible: {error}")
        } else if startup_hidden {
            String::from("Start hidden is enabled, but this launch starts visible for safety")
        } else {
            String::from("Ready")
        };

        let selected_id = store.history().first().map(|entry| entry.id.clone());
        let mut app = Self {
            store,
            search: String::new(),
            selected_id,
            status_message: initial_status,
            settings,
            show_settings: false,
            hotkey_manager,
            registered_hotkey: None,
            tray,
            tray_error,
            window_visible: true,
            quit_requested: false,
            startup_hide_pending,
            start_hidden_requested: startup_hidden,
            start_hidden_effective: startup_hidden && tray_available,
            settings_hotkey_input,
            settings_start_hidden,
            capturing_hotkey: false,
            search_has_focus: false,
            runtime_shared,
        };
        app.apply_hotkey_setting();
        if app.tray.is_none() {
            app.status_message =
                String::from("Tray icon unavailable, starting visible for safety");
        } else if app.start_hidden_requested {
            app.status_message =
                String::from("Start hidden is enabled, but this launch starts visible for safety");
        }
        app
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
            append_log(format!(
                "copy_selected attempt: id={} kind={:?} preview={}",
                entry.id,
                entry.kind,
                truncate(&entry.preview, 48)
            ));
            match self.store.copy_entry(&entry.id) {
                Ok(()) => {
                    append_log(format!("copy_selected success: id={}", entry.id));
                    self.status_message =
                        format!("Copied '{}' back to clipboard", truncate(&entry.preview, 36));
                }
                Err(error) => {
                    append_log(format!("copy_selected failed: id={} error={error}", entry.id));
                    self.status_message = error;
                }
            }
        } else {
            append_log("copy_selected skipped: no selected entry");
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

    fn handle_shortcuts(&mut self, ctx: &Context) {
        if self.show_settings || self.capturing_hotkey || self.search_has_focus {
            append_log(format!(
                "ctrl+c skipped: settings_open={} capturing_hotkey={} search_has_focus={}",
                self.show_settings, self.capturing_hotkey, self.search_has_focus
            ));
            return;
        }

        let copy_pressed = ctx.input_mut(|input| {
            input.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::C))
                || input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::C))
        });
        let copy_event_detected = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        });

        if copy_pressed || copy_event_detected {
            append_log(format!(
                "ctrl+c detected: selected_id={} shortcut={} event_copy={}",
                self.selected_id
                    .as_deref()
                    .unwrap_or("<none>"),
                copy_pressed,
                copy_event_detected
            ));
            let items = self.filtered_history();
            self.copy_selected(&items);
        }
    }

    fn show_window(&mut self, ctx: &Context) {
        append_log("show_window");
        self.window_visible = true;
        self.runtime_shared
            .window_visible
            .store(true, Ordering::SeqCst);
        apply_window_visibility(ctx, &self.runtime_shared, true);
        self.status_message = String::from("Vclipboard shown");
    }

    fn hide_window(&mut self, ctx: &Context) {
        if self.tray.is_none() {
            append_log("hide_window blocked: tray unavailable");
            self.window_visible = true;
            self.runtime_shared
                .window_visible
                .store(true, Ordering::SeqCst);
            self.status_message =
                String::from("Tray icon unavailable, cannot hide window to tray");
            return;
        }
        append_log("hide_window to tray");
        self.window_visible = false;
        self.runtime_shared
            .window_visible
            .store(false, Ordering::SeqCst);
        apply_window_visibility(ctx, &self.runtime_shared, false);
        self.status_message = String::from("Vclipboard hidden to tray");
    }

    fn apply_hotkey_setting(&mut self) {
        let Some(manager) = self.hotkey_manager.as_ref() else {
            append_log("apply_hotkey_setting: manager unavailable");
            self.status_message = String::from("Global hotkey is not available on this system");
            if self.start_hidden_requested {
                self.start_hidden_effective = false;
                self.startup_hide_pending = false;
            }
            return;
        };

        if let Some(current) = self.registered_hotkey.take() {
            let _ = manager.unregister(current);
        }

        match parse_hotkey_setting(&self.settings.hotkey) {
            Ok(Some(hotkey)) => match manager.register(hotkey.clone()) {
                Ok(()) => {
                    append_log(format!("hotkey registered: {}", self.settings.hotkey));
                    self.registered_hotkey = Some(hotkey);
                    self.runtime_shared.hotkey_id.store(
                        self.registered_hotkey.as_ref().map(|h| h.id()).unwrap_or(0),
                        Ordering::SeqCst,
                    );
                    let _ = save_settings(&settings_path(), &self.settings);
                    self.status_message = format!("Hotkey set to {}", self.settings.hotkey.as_str());
                }
                Err(error) => {
                    append_log(format!(
                        "hotkey register failed: {} | {}",
                        self.settings.hotkey, error
                    ));
                    self.start_hidden_effective = false;
                    self.startup_hide_pending = false;
                    self.runtime_shared.hotkey_id.store(0, Ordering::SeqCst);
                    self.status_message = format!(
                        "Could not register hotkey '{}': {error}. Starting visible for safety",
                        self.settings.hotkey
                    );
                }
            },
            Ok(None) => {
                append_log("hotkey disabled");
                self.runtime_shared.hotkey_id.store(0, Ordering::SeqCst);
                let _ = save_settings(&settings_path(), &self.settings);
                self.status_message = String::from("Global hotkey disabled");
            }
            Err(error) => {
                append_log(format!("hotkey parse failed: {error}"));
                self.start_hidden_effective = false;
                self.startup_hide_pending = false;
                self.runtime_shared.hotkey_id.store(0, Ordering::SeqCst);
                self.status_message = error;
            }
        }
    }

    fn sync_runtime_requests(&mut self) {
        self.window_visible = self.runtime_shared.window_visible.load(Ordering::SeqCst);
        if self
            .runtime_shared
            .open_settings
            .swap(false, Ordering::SeqCst)
        {
            self.settings_hotkey_input = self.settings.hotkey.clone();
            self.settings_start_hidden = self.settings.start_hidden_in_tray;
            self.show_settings = true;
        }
    }

    fn settings_window(&mut self, ctx: &Context) {
        if !self.show_settings {
            return;
        }

        if self.capturing_hotkey {
            if let Some(recorded) = record_hotkey_from_input(ctx) {
                self.settings_hotkey_input = recorded;
                self.capturing_hotkey = false;
                append_log(format!(
                    "hotkey recorded from keyboard: {}",
                    self.settings_hotkey_input
                ));
            }
        }

        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label("Global hotkey to show / hide Vclipboard");
                let record_label = if self.capturing_hotkey {
                    format!("Press shortcut... [{}]", self.settings_hotkey_input)
                } else if self.settings_hotkey_input.trim().is_empty() {
                    String::from("Click to record shortcut")
                } else {
                    self.settings_hotkey_input.clone()
                };
                let response = ui.add_sized([220.0, 28.0], Button::new(record_label));
                if response.clicked() {
                    self.capturing_hotkey = true;
                }
                if self.capturing_hotkey {
                    ui.label(
                        RichText::new("Recording... press a key combo, or Esc to cancel")
                            .size(11.0)
                            .color(Color32::from_rgb(70, 90, 150)),
                    );
                }
                ui.add_space(4.0);
                ui.add_enabled_ui(self.tray.is_some(), |ui| {
                    ui.checkbox(&mut self.settings_start_hidden, "Start hidden in tray");
                });
                if let Some(error) = self.tray_error.as_ref() {
                    ui.label(
                        RichText::new(format!("Tray unavailable: {error}"))
                            .size(11.0)
                            .color(Color32::from_rgb(150, 72, 52)),
                    );
                } else if self.start_hidden_requested && !self.start_hidden_effective {
                    ui.label(
                        RichText::new("Start hidden was skipped for safety")
                            .size(11.0)
                            .color(Color32::from_rgb(150, 72, 52)),
                    );
                }
                ui.label(
                    RichText::new("Examples: Ctrl+Shift+V, Ctrl+Alt+V, Alt+Space, F8")
                        .size(11.0)
                        .color(Color32::from_rgb(88, 88, 88)),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.settings.hotkey = self.settings_hotkey_input.trim().to_string();
                        self.settings.start_hidden_in_tray =
                            self.settings_start_hidden && self.tray.is_some();
                        self.start_hidden_requested = self.settings.start_hidden_in_tray;
                        self.start_hidden_effective =
                            self.start_hidden_requested && self.tray.is_some();
                        self.startup_hide_pending = false;
                        self.apply_hotkey_setting();
                        self.show_settings = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.settings = load_settings(&settings_path());
                        self.settings_hotkey_input = self.settings.hotkey.clone();
                        self.settings_start_hidden = self.settings.start_hidden_in_tray;
                        self.capturing_hotkey = false;
                        self.show_settings = false;
                    }
                });
            });
        self.show_settings = open;
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
                    if ui.add(Button::new("Settings")).clicked() {
                        self.settings_hotkey_input = self.settings.hotkey.clone();
                        self.settings_start_hidden = self.settings.start_hidden_in_tray;
                        self.capturing_hotkey = false;
                        self.show_settings = true;
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
        let selected_summary = self
            .selected_entry(&self.filtered_history())
            .map(|entry| match entry.kind {
                ClipboardEntryKind::Text => {
                    format!("{} chars | {} lines", entry.character_count, entry.line_count)
                }
                ClipboardEntryKind::Image => format!(
                    "Image {}x{}",
                    entry.image_width.unwrap_or_default(),
                    entry.image_height.unwrap_or_default()
                ),
            })
            .unwrap_or_else(|| String::from("No selection"));

        TopBottomPanel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(240, 240, 240))
                    .inner_margin(Margin::symmetric(8, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let search_response = ui.add_sized(
                        [140.0, 22.0],
                        TextEdit::singleline(&mut self.search).hint_text("Search clips..."),
                    );
                    self.search_has_focus = search_response.has_focus();
                    ui.separator();
                    ui.label(
                        RichText::new(format!("{visible_count}/{total_count} clips"))
                            .color(Color32::from_rgb(50, 50, 50)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(selected_summary).color(Color32::from_rgb(55, 55, 55)),
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

        Frame::group(ui.style())
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    // Căn trái toàn bộ cell trong bảng
                    .cell_layout(Layout::left_to_right(Align::Min))
                    .column(Column::exact(18.0))
                    .column(Column::remainder());

                if !compact {
                    table = table.column(Column::exact(92.0));
                }

                table
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new("#")
                                        .strong()
                                        .color(Color32::from_rgb(48, 48, 48)),
                                )
                                .halign(Align::Min),
                            );
                        });
                        header.col(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new("Clipboard history")
                                        .strong()
                                        .color(Color32::from_rgb(48, 48, 48)),
                                )
                                .halign(Align::Min),
                            );
                        });
                        if !compact {
                            header.col(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new("Captured")
                                            .strong()
                                            .color(Color32::from_rgb(48, 48, 48)),
                                    )
                                    .halign(Align::Min),
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
                            row.set_selected(is_selected);

                            // Cột icon
                            row.col(|ui| {
                                let icon = match entry.kind {
                                    ClipboardEntryKind::Image => "I",
                                    ClipboardEntryKind::Text if entry.line_count > 1 => "S",
                                    ClipboardEntryKind::Text => "T",
                                };
                                let text = RichText::new(icon)
                                    .strong()
                                    .color(if is_selected {
                                        Color32::from_rgb(205, 225, 255)
                                    } else {
                                        Color32::from_rgb(0, 70, 160)
                                    });
                                let response = ui
                                    .add(
                                        egui::Label::new(text)
                                            .halign(Align::Min)
                                            .sense(Sense::click())
                                            .selectable(false),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if response.clicked() {
                                    ui.ctx().memory_mut(|mem| mem.stop_text_input());
                                    append_log(format!("row icon clicked: id={}", entry.id));
                                    self.selected_id = Some(entry.id.clone());
                                }
                            });

                            // Cột preview (nội dung chính)
                            row.col(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let preview_text = if compact {
                                    truncate(&entry.preview, 40)
                                } else {
                                    truncate(&entry.preview, 86)
                                };
                                let row_text = RichText::new(preview_text).color(if is_selected {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgb(38, 38, 38)
                                });

                                let response = ui
                                    .allocate_ui_with_layout(
                                        Vec2::new(ui.available_width(), 18.0),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(row_text)
                                                    .truncate()
                                                    .halign(Align::Min)
                                                    .selectable(false)
                                                    .sense(Sense::click()),
                                            )
                                        },
                                    )
                                    .inner
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);

                                if response.clicked() {
                                    ui.ctx().memory_mut(|mem| mem.stop_text_input());
                                    append_log(format!("row clicked: id={}", entry.id));
                                    self.selected_id = Some(entry.id.clone());
                                }
                                if response.double_clicked() {
                                    append_log(format!("row double clicked: id={}", entry.id));
                                    match self.store.copy_entry(&entry.id) {
                                        Ok(()) => {
                                            append_log(format!(
                                                "row double click copy success: id={}",
                                                entry.id
                                            ));
                                            self.status_message = format!(
                                                "Copied '{}' back to clipboard",
                                                truncate(&entry.preview, 36)
                                            );
                                        }
                                        Err(error) => {
                                            append_log(format!(
                                                "row double click copy failed: id={} error={error}",
                                                entry.id
                                            ));
                                            self.status_message = error;
                                        }
                                    }
                                }
                                response.clone().context_menu(|ui| {
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

                            // Cột thời gian
                            if !compact {
                                row.col(|ui| {
                                    let color = if is_selected {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_gray(90)
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format_timestamp(entry.created_at)).color(color),
                                        )
                                        .halign(Align::Min),
                                    );
                                });
                            }

                            let _ = row.response().on_hover_text(&entry.preview);
                        });
                    });

                if total_items == 0 {
                    ui.add_space(8.0);
                    ui.label("Clipboard history is empty. Copy any text in Windows to start.");
                }
            });
    }
}

impl App for ClipboardDiaryApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.sync_runtime_requests();
        self.handle_shortcuts(ctx);

        if self.startup_hide_pending {
            append_log("startup_hide_pending fired");
            self.startup_hide_pending = false;
            if self.start_hidden_requested && self.start_hidden_effective && self.tray.is_some() {
                self.hide_window(ctx);
            } else {
                self.window_visible = true;
                if self.tray.is_none() {
                    self.status_message =
                        String::from("Start hidden skipped for safety because tray is unavailable");
                }
            }
        }

        if ctx.input(|input| input.viewport().close_requested()) && !self.quit_requested {
            append_log("viewport close requested");
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if self.tray.is_some() {
                self.hide_window(ctx);
            } else {
                self.status_message =
                    String::from("Tray icon unavailable, keeping window visible");
                self.show_window(ctx);
            }
        }

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

        self.settings_window(ctx);
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

