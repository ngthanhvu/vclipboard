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
use tauri::{AppHandle, Emitter, Manager, State};

const MAX_HISTORY_ITEMS: usize = 250;
const POLL_INTERVAL_MS: u64 = 900;
const CLIPBOARD_EVENT: &str = "clipboard://updated";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardEntry {
    id: String,
    content: String,
    preview: String,
    created_at: u64,
    character_count: usize,
    line_count: usize,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardSnapshot {
    items: Vec<ClipboardEntry>,
}

struct AppState {
    items: Mutex<Vec<ClipboardEntry>>,
    storage_path: PathBuf,
    monitor_started: AtomicBool,
}

impl AppState {
    fn new(storage_path: PathBuf) -> Self {
        let items = load_history(&storage_path);
        Self {
            items: Mutex::new(items),
            storage_path,
            monitor_started: AtomicBool::new(false),
        }
    }

    fn get_items(&self) -> Vec<ClipboardEntry> {
        self.items
            .lock()
            .map(|items| items.clone())
            .unwrap_or_default()
    }

    fn start_monitor(self: &Arc<Self>, app: AppHandle) {
        if self.monitor_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let state = Arc::clone(self);
        thread::spawn(move || {
            let mut last_clipboard = state
                .items
                .lock()
                .ok()
                .and_then(|items| items.first().map(|entry| entry.content.clone()))
                .unwrap_or_default();

            loop {
                match read_clipboard_text() {
                    Ok(content) => {
                        if content.trim().is_empty() || content == last_clipboard {
                            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                            continue;
                        }

                        if let Some(entry) = state.record_content(content.clone()) {
                            last_clipboard = content;
                            let payload = ClipboardSnapshot {
                                items: vec![entry],
                            };
                            let _ = app.emit(CLIPBOARD_EVENT, payload);
                        }
                    }
                    Err(_) => {}
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

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn app_storage_path(app: &AppHandle) -> PathBuf {
    let base_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let _ = fs::create_dir_all(&base_dir);
    base_dir.join("clipboard-history.json")
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

#[tauri::command]
fn get_history(state: State<'_, Arc<AppState>>) -> Vec<ClipboardEntry> {
    state.get_items()
}

#[tauri::command]
fn copy_entry(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let items = state.get_items();
    let entry = items
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| String::from("Khong tim thay muc clipboard"))?;

    write_clipboard_text(&entry.content)?;
    let _ = state.record_content(entry.content);
    Ok(())
}

#[tauri::command]
fn delete_entry(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.delete_entry(&id)
}

#[tauri::command]
fn clear_history(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.clear()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let storage_path = app_storage_path(&app.handle().clone());
            let state = Arc::new(AppState::new(storage_path));
            state.start_monitor(app.handle().clone());
            app.manage(state);
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_history,
            copy_entry,
            delete_entry,
            clear_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
