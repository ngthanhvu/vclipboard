use arboard::Clipboard;
use chrono::{Local, TimeZone};
use directories::ProjectDirs;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::types::{AppSettings, ClipboardEntry};

pub(crate) fn build_entry(content: String) -> ClipboardEntry {
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

pub(crate) fn build_preview(content: &str) -> String {
    let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview: String = collapsed.chars().take(120).collect();
    if collapsed.chars().count() > 120 {
        format!("{preview}...")
    } else {
        preview
    }
}

fn app_data_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "ngthanhvu", "Vclipboard") {
        let base_dir = project_dirs.data_dir();
        let _ = fs::create_dir_all(base_dir);
        return base_dir.to_path_buf();
    }

    PathBuf::from(".")
}

pub(crate) fn app_storage_path() -> PathBuf {
    app_data_dir().join("clipboard-history.json")
}

pub(crate) fn settings_path() -> PathBuf {
    app_data_dir().join("settings.json")
}

fn log_path() -> PathBuf {
    app_data_dir().join("runtime.log")
}

pub(crate) fn append_log(message: impl AsRef<str>) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("[{timestamp}] {}\n", message.as_ref());
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = file.write_all(line.as_bytes());
    }
}

pub(crate) fn load_history(path: &Path) -> Vec<ClipboardEntry> {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<Vec<ClipboardEntry>>(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub(crate) fn save_history(path: &Path, items: &[ClipboardEntry]) {
    if let Ok(json) = serde_json::to_string_pretty(items) {
        let _ = fs::write(path, json);
    }
}

pub(crate) fn load_settings(path: &Path) -> AppSettings {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<AppSettings>(&content).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}

pub(crate) fn save_settings(path: &Path, settings: &AppSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(path, json).map_err(|error| error.to_string())
}

pub(crate) fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .get_text()
        .map(|text| text.replace("\r\n", "\n").trim_end_matches('\n').to_string())
        .map_err(|error| error.to_string())
}

pub(crate) fn write_clipboard_text(content: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_text(content.to_string())
        .map_err(|error| error.to_string())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn format_timestamp(timestamp: u64) -> String {
    match Local.timestamp_millis_opt(timestamp as i64).single() {
        Some(date_time) => date_time.format("%H:%M:%S %d-%m").to_string(),
        None => String::from("--:--:--"),
    }
}

pub(crate) fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}
