use eframe::egui::Context;
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
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
    build_entry, cleanup_orphaned_image_assets, clear_image_assets_dir, delete_entry_assets,
    load_history, read_clipboard_content, save_history, write_clipboard_entry,
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
    #[serde(default)]
    pub(crate) kind: ClipboardEntryKind,
    #[serde(default)]
    pub(crate) image_path: Option<String>,
    #[serde(default)]
    pub(crate) image_width: Option<usize>,
    #[serde(default)]
    pub(crate) image_height: Option<usize>,
    #[serde(default)]
    pub(crate) content_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum ClipboardEntryKind {
    #[default]
    Text,
    Image,
}

#[derive(Debug, Clone)]
pub(crate) enum ClipboardContent {
    Text {
        text: String,
    },
    Image {
        width: usize,
        height: usize,
        rgba_bytes: Vec<u8>,
    },
}

impl ClipboardContent {
    pub(crate) fn signature(&self) -> String {
        match self {
            Self::Text { text } => format!("text:{text}"),
            Self::Image {
                width,
                height,
                rgba_bytes,
                ..
            } => {
                let mut hasher = DefaultHasher::new();
                rgba_bytes.hash(&mut hasher);
                format!("image:{width}x{height}:{}", hasher.finish())
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::Text { text } => text.trim().is_empty(),
            Self::Image {
                width,
                height,
                rgba_bytes,
                ..
            } => *width == 0 || *height == 0 || rgba_bytes.is_empty(),
        }
    }
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
        let items = load_history(&storage_path);
        cleanup_orphaned_image_assets(&items);
        Self {
            items: Mutex::new(items),
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
                .and_then(|items| items.first().map(|entry| entry.content_signature.clone()))
                .unwrap_or_default();

            loop {
                if let Ok(content) = read_clipboard_content() {
                    let signature = content.signature();
                    if !content.is_empty() && signature != last_clipboard {
                        if let Some(entry) = store.record_content(content) {
                            last_clipboard = entry.content_signature.clone();
                            ctx.request_repaint();
                        }
                    }
                }

                thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
        });
    }

    pub(crate) fn record_content(&self, content: ClipboardContent) -> Option<ClipboardEntry> {
        let entry = build_entry(content);
        let mut items = self.items.lock().ok()?;

        if items
            .first()
            .map(|current| current.content_signature == entry.content_signature)
            .unwrap_or(false)
        {
            return None;
        }

        let removed: Vec<ClipboardEntry> = items
            .iter()
            .filter(|item| item.content_signature == entry.content_signature)
            .cloned()
            .collect();
        items.retain(|item| item.content_signature != entry.content_signature);
        for item in removed {
            delete_entry_assets(&item);
        }
        items.insert(0, entry.clone());
        if items.len() > MAX_HISTORY_ITEMS {
            let overflow = items.split_off(MAX_HISTORY_ITEMS);
            for item in overflow {
                delete_entry_assets(&item);
            }
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

        write_clipboard_entry(&entry)?;
        Ok(())
    }

    pub(crate) fn delete_entry(&self, id: &str) -> Result<(), String> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| String::from("Khong the truy cap lich su clipboard"))?;
        let original_len = items.len();
        let removed: Vec<ClipboardEntry> = items
            .iter()
            .filter(|item| item.id == id)
            .cloned()
            .collect();
        items.retain(|item| item.id != id);
        if items.len() == original_len {
            return Err(String::from("Khong tim thay muc can xoa"));
        }
        for item in removed {
            delete_entry_assets(&item);
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
        clear_image_assets_dir();
        save_history(&self.storage_path, &items);
        Ok(())
    }
}
