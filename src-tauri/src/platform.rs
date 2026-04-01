use eframe::{
    icon_data::from_png_bytes,
    egui::{self, Context, IconData, ViewportCommand},
};
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent};
use std::{
    sync::{
        atomic::Ordering,
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem},
    Icon, MouseButton, TrayIconBuilder, TrayIconEvent,
};

use crate::{
    storage::append_log,
    types::{RuntimeShared, TrayHandles},
};

pub(crate) fn parse_hotkey_setting(value: &str) -> Result<Option<HotKey>, String> {
    let normalized = value.trim();
    if normalized.is_empty() || normalized.eq_ignore_ascii_case("disabled") {
        return Ok(None);
    }

    normalized
        .parse::<HotKey>()
        .map(Some)
        .map_err(|error| format!("Invalid hotkey '{normalized}': {error}"))
}

pub(crate) fn record_hotkey_from_input(ctx: &Context) -> Option<String> {
    let events = ctx.input(|input| input.events.clone());
    let modifiers = ctx.input(|input| input.modifiers);

    for event in events {
        if let egui::Event::Key { key, pressed, .. } = event {
            if !pressed {
                continue;
            }

            if key == egui::Key::Escape {
                return Some(String::new());
            }

            if let Some(main_key) = format_egui_key(key) {
                let mut parts: Vec<&str> = Vec::new();
                if modifiers.ctrl {
                    parts.push("Ctrl");
                }
                if modifiers.shift {
                    parts.push("Shift");
                }
                if modifiers.alt {
                    parts.push("Alt");
                }
                if modifiers.mac_cmd || modifiers.command {
                    parts.push("Super");
                }
                parts.push(main_key);
                return Some(parts.join("+"));
            }
        }
    }

    None
}

fn format_egui_key(key: egui::Key) -> Option<&'static str> {
    use egui::Key;

    match key {
        Key::ArrowDown => Some("Down"),
        Key::ArrowLeft => Some("Left"),
        Key::ArrowRight => Some("Right"),
        Key::ArrowUp => Some("Up"),
        Key::Escape => None,
        Key::Tab => Some("Tab"),
        Key::Backspace => Some("Backspace"),
        Key::Enter => Some("Enter"),
        Key::Space => Some("Space"),
        Key::Insert => Some("Insert"),
        Key::Delete => Some("Delete"),
        Key::Home => Some("Home"),
        Key::End => Some("End"),
        Key::PageUp => Some("PageUp"),
        Key::PageDown => Some("PageDown"),
        Key::Num0 => Some("0"),
        Key::Num1 => Some("1"),
        Key::Num2 => Some("2"),
        Key::Num3 => Some("3"),
        Key::Num4 => Some("4"),
        Key::Num5 => Some("5"),
        Key::Num6 => Some("6"),
        Key::Num7 => Some("7"),
        Key::Num8 => Some("8"),
        Key::Num9 => Some("9"),
        Key::A => Some("A"),
        Key::B => Some("B"),
        Key::C => Some("C"),
        Key::D => Some("D"),
        Key::E => Some("E"),
        Key::F => Some("F"),
        Key::G => Some("G"),
        Key::H => Some("H"),
        Key::I => Some("I"),
        Key::J => Some("J"),
        Key::K => Some("K"),
        Key::L => Some("L"),
        Key::M => Some("M"),
        Key::N => Some("N"),
        Key::O => Some("O"),
        Key::P => Some("P"),
        Key::Q => Some("Q"),
        Key::R => Some("R"),
        Key::S => Some("S"),
        Key::T => Some("T"),
        Key::U => Some("U"),
        Key::V => Some("V"),
        Key::W => Some("W"),
        Key::X => Some("X"),
        Key::Y => Some("Y"),
        Key::Z => Some("Z"),
        Key::F1 => Some("F1"),
        Key::F2 => Some("F2"),
        Key::F3 => Some("F3"),
        Key::F4 => Some("F4"),
        Key::F5 => Some("F5"),
        Key::F6 => Some("F6"),
        Key::F7 => Some("F7"),
        Key::F8 => Some("F8"),
        Key::F9 => Some("F9"),
        Key::F10 => Some("F10"),
        Key::F11 => Some("F11"),
        Key::F12 => Some("F12"),
        _ => None,
    }
}

pub(crate) fn apply_window_visibility(ctx: &Context, visible: bool) {
    append_log(format!("apply_window_visibility visible={visible}"));
    if visible {
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
    } else {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

pub(crate) fn create_tray() -> Result<TrayHandles, String> {
    append_log("create_tray start");
    let menu = Menu::new();
    let show_hide = MenuItem::new("Show / Hide", true, None);
    let settings = MenuItem::new("Settings", true, None);
    let quit = MenuItem::new("Quit", true, None);

    menu.append(&show_hide).map_err(|error| error.to_string())?;
    menu.append(&settings).map_err(|error| error.to_string())?;
    menu.append(&quit).map_err(|error| error.to_string())?;

    let tray_icon = load_tray_icon()?;
    let icon = Icon::from_rgba(tray_icon.rgba, tray_icon.width, tray_icon.height)
        .map_err(|error| error.to_string())?;
    let tray = TrayIconBuilder::new()
        .with_tooltip("Vclipboard")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .build()
        .map_err(|error| error.to_string())?;

    append_log("create_tray success");

    Ok(TrayHandles {
        _tray: tray,
        show_hide_id: show_hide.id().clone(),
        settings_id: settings.id().clone(),
        quit_id: quit.id().clone(),
    })
}

pub(crate) fn spawn_external_event_forwarders(
    ctx: Context,
    shared: Arc<RuntimeShared>,
    show_hide_id: MenuId,
    settings_id: MenuId,
    quit_id: MenuId,
) {
    let menu_ctx = ctx.clone();
    let menu_shared = Arc::clone(&shared);
    thread::spawn(move || loop {
        match MenuEvent::receiver().recv() {
            Ok(event) => {
                append_log(format!("tray menu event: {}", event.id.0));
                if event.id == show_hide_id {
                    if should_toggle_now(&menu_shared.last_tray_toggle) {
                        let currently_visible = menu_shared.window_visible.load(Ordering::SeqCst);
                        let next_visible = !currently_visible;
                        append_log(format!("tray menu toggle -> visible={next_visible}"));
                        menu_shared
                            .window_visible
                            .store(next_visible, Ordering::SeqCst);
                        apply_window_visibility(&menu_ctx, next_visible);
                    } else {
                        append_log("tray menu toggle ignored (debounced)");
                    }
                } else if event.id == settings_id {
                    menu_shared.window_visible.store(true, Ordering::SeqCst);
                    menu_shared.open_settings.store(true, Ordering::SeqCst);
                    append_log("tray menu settings -> visible=true");
                    apply_window_visibility(&menu_ctx, true);
                } else if event.id == quit_id {
                    append_log("tray menu quit");
                    menu_ctx.send_viewport_cmd(ViewportCommand::Close);
                    break;
                }
                menu_ctx.request_repaint();
            }
            Err(_) => break,
        }
    });

    let click_ctx = ctx.clone();
    let click_shared = Arc::clone(&shared);
    thread::spawn(move || loop {
        match TrayIconEvent::receiver().recv() {
            Ok(TrayIconEvent::DoubleClick { button, .. }) => {
                append_log(format!("tray double click: {:?}", button));
                if button == MouseButton::Left {
                    if should_toggle_now(&click_shared.last_tray_toggle) {
                        let currently_visible = click_shared.window_visible.load(Ordering::SeqCst);
                        let next_visible = !currently_visible;
                        append_log(format!("tray double click toggle -> visible={next_visible}"));
                        click_shared
                            .window_visible
                            .store(next_visible, Ordering::SeqCst);
                        apply_window_visibility(&click_ctx, next_visible);
                        click_ctx.request_repaint();
                    } else {
                        append_log("tray double click ignored (debounced)");
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    });

    thread::spawn(move || loop {
        match GlobalHotKeyEvent::receiver().recv() {
            Ok(event) => {
                append_log(format!("global hotkey event: {}", event.id));
                let registered_id = shared.hotkey_id.load(Ordering::SeqCst);
                if registered_id != 0 && registered_id == event.id {
                    if should_toggle_now(&shared.last_hotkey_toggle) {
                        let currently_visible = shared.window_visible.load(Ordering::SeqCst);
                        let next_visible = !currently_visible;
                        append_log(format!("hotkey toggle -> visible={next_visible}"));
                        shared.window_visible.store(next_visible, Ordering::SeqCst);
                        apply_window_visibility(&ctx, next_visible);
                        ctx.request_repaint();
                    } else {
                        append_log("hotkey toggle ignored (debounced)");
                    }
                }
            }
            Err(_) => break,
        }
    });
}

fn should_toggle_now(last_toggle: &Mutex<Option<Instant>>) -> bool {
    let mut guard = match last_toggle.lock() {
        Ok(guard) => guard,
        Err(_) => return true,
    };

    let now = Instant::now();
    if let Some(previous) = *guard {
        if now.duration_since(previous) < Duration::from_millis(300) {
            return false;
        }
    }

    *guard = Some(now);
    true
}

fn load_tray_icon() -> Result<IconData, String> {
    from_png_bytes(include_bytes!("../../assets/16x16.png")).map_err(|error| error.to_string())
}

pub(crate) fn load_window_icon() -> Result<IconData, String> {
    from_png_bytes(include_bytes!("../../assets/256x256.png")).map_err(|error| error.to_string())
}
