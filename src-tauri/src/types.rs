use eframe::egui::Context;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tray_icon::{menu::MenuId, TrayIcon};

use crate::storage::{
    build_entry, load_history, read_clipboard_text, save_history, write_clipboard_text,
};

pub(crate) const MAX_HISTORY_ITEMS: usize = 250;
pub(crate) const POLL_INTERVAL_MS: u64 = 900;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClipboardEntry {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) preview: String,
    pub(crate) created_at: u64,
    pub(crate) character_count: usize,
    pub(crate) line_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    pub(crate) hotkey: String,
    pub(crate) start_hidden_in_tray: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: String::from("Ctrl+Shift+V"),
            start_hidden_in_tray: false,
        }
    }
}

pub(crate) struct TrayHandles {
    pub(crate) _tray: TrayIcon,
    pub(crate) show_hide_id: MenuId,
    pub(crate) settings_id: MenuId,
    pub(crate) quit_id: MenuId,
}

pub(crate) struct RuntimeShared {
    pub(crate) window_visible: AtomicBool,
    pub(crate) native_hwnd: AtomicIsize,
    pub(crate) hotkey_id: AtomicU32,
    pub(crate) open_settings: AtomicBool,
    pub(crate) last_hotkey_toggle: Mutex<Option<Instant>>,
    pub(crate) last_tray_toggle: Mutex<Option<Instant>>,
}

pub(crate) struct HistoryStore {
    items: Mutex<Vec<ClipboardEntry>>,
    storage_path: PathBuf,
    monitor_started: AtomicBool,
}

impl HistoryStore {
    pub(crate) fn new(storage_path: PathBuf) -> Self {
        Self {
            items: Mutex::new(load_history(&storage_path)),
            storage_path,
            monitor_started: AtomicBool::new(false),
        }
    }

    pub(crate) fn history(&self) -> Vec<ClipboardEntry> {
        self.items
            .lock()
            .map(|items| items.clone())
            .unwrap_or_default()
    }

    pub(crate) fn start_monitor(self: &Arc<Self>, ctx: Context) {
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

    pub(crate) fn record_content(&self, content: String) -> Option<ClipboardEntry> {
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

    pub(crate) fn copy_entry(&self, id: &str) -> Result<(), String> {
        let items = self.history();
        let entry = items
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| String::from("Khong tim thay clip can copy"))?;

        write_clipboard_text(&entry.content)?;
        let _ = self.record_content(entry.content);
        Ok(())
    }

    pub(crate) fn delete_entry(&self, id: &str) -> Result<(), String> {
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

    pub(crate) fn clear(&self) -> Result<(), String> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| String::from("Khong the truy cap lich su clipboard"))?;
        items.clear();
        save_history(&self.storage_path, &items);
        Ok(())
    }
}
