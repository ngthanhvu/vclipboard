use arboard::{Clipboard, ImageData};
use chrono::{Local, TimeZone};
use directories::ProjectDirs;
use image::{ColorType, ImageEncoder};
use std::{
    borrow::Cow,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::types::{AppSettings, ClipboardContent, ClipboardEntry, ClipboardEntryKind};

pub(crate) fn build_entry(content: ClipboardContent) -> ClipboardEntry {
    let created_at = now_millis();
    match content {
        ClipboardContent::Text { text } => {
            let preview = build_preview(&text);
            let character_count = text.chars().count();
            let line_count = text.lines().count().max(1);

            ClipboardEntry {
                id: format!("{created_at}-{character_count}"),
                content_signature: format!("text:{text}"),
                content: text,
                preview,
                created_at,
                character_count,
                line_count,
                kind: ClipboardEntryKind::Text,
                image_path: None,
                image_width: None,
                image_height: None,
            }
        }
        ClipboardContent::Image {
            png_relative_path,
            width,
            height,
            rgba_bytes,
        } => ClipboardEntry {
            id: format!("{created_at}-img-{width}x{height}"),
            content: String::new(),
            preview: format!("Image {width}x{height}"),
            created_at,
            character_count: rgba_bytes.len(),
            line_count: 1,
            kind: ClipboardEntryKind::Image,
            image_path: Some(png_relative_path),
            image_width: Some(width),
            image_height: Some(height),
            content_signature: ClipboardContent::Image {
                png_relative_path: String::new(),
                width,
                height,
                rgba_bytes,
            }
            .signature(),
        },
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

fn images_dir() -> PathBuf {
    let dir = app_data_dir().join("images");
    let _ = fs::create_dir_all(&dir);
    dir
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
        Ok(content) => serde_json::from_str::<Vec<ClipboardEntry>>(&content)
            .unwrap_or_default()
            .into_iter()
            .map(normalize_loaded_entry)
            .collect(),
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

pub(crate) fn read_clipboard_content() -> Result<ClipboardContent, String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    if let Ok(text) = clipboard.get_text() {
        return Ok(ClipboardContent::Text {
            text: text
                .replace("\r\n", "\n")
                .trim_end_matches('\n')
                .to_string(),
        });
    }

    let image = clipboard.get_image().map_err(|error| error.to_string())?;
    let rgba_bytes = image.bytes.into_owned();
    let relative_path = save_clipboard_image(image.width, image.height, &rgba_bytes)?;
    Ok(ClipboardContent::Image {
        png_relative_path: relative_path,
        width: image.width,
        height: image.height,
        rgba_bytes,
    })
}

pub(crate) fn write_clipboard_entry(entry: &ClipboardEntry) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    match entry.kind {
        ClipboardEntryKind::Text => clipboard
            .set_text(entry.content.to_string())
            .map_err(|error| error.to_string()),
        ClipboardEntryKind::Image => {
            let image_path = entry
                .image_path
                .as_ref()
                .ok_or_else(|| String::from("Khong tim thay file anh da luu"))?;
            let bytes = fs::read(app_data_dir().join(image_path)).map_err(|error| error.to_string())?;
            let image = image::load_from_memory(&bytes)
                .map_err(|error| error.to_string())?
                .into_rgba8();
            let (width, height) = image.dimensions();
            clipboard
                .set_image(ImageData {
                    width: width as usize,
                    height: height as usize,
                    bytes: Cow::Owned(image.into_raw()),
                })
                .map_err(|error| error.to_string())
        }
    }
}

pub(crate) fn delete_entry_assets(entry: &ClipboardEntry) {
    if let Some(path) = entry.image_path.as_ref() {
        let _ = fs::remove_file(app_data_dir().join(path));
    }
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

fn save_clipboard_image(width: usize, height: usize, rgba_bytes: &[u8]) -> Result<String, String> {
    let file_name = format!("clip-{}-{}x{}.png", now_millis(), width, height);
    let full_path = images_dir().join(&file_name);
    let file = fs::File::create(&full_path).map_err(|error| error.to_string())?;
    let encoder = image::codecs::png::PngEncoder::new(file);
    encoder
        .write_image(
            rgba_bytes,
            width as u32,
            height as u32,
            ColorType::Rgba8.into(),
        )
        .map_err(|error| error.to_string())?;
    Ok(PathBuf::from("images")
        .join(file_name)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn normalize_loaded_entry(mut entry: ClipboardEntry) -> ClipboardEntry {
    if entry.kind == ClipboardEntryKind::Image {
        if let (Some(width), Some(height)) = (entry.image_width, entry.image_height) {
            if entry.preview.is_empty() {
                entry.preview = format!("Image {width}x{height}");
            }
            if entry.content_signature.is_empty() {
                entry.content_signature = format!(
                    "image:{}:{}:{}",
                    width,
                    height,
                    entry.image_path.clone().unwrap_or_default()
                );
            }
        }
    } else if entry.content_signature.is_empty() {
        entry.content_signature = format!("text:{}", entry.content);
    }

    entry
}
