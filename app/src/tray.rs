use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::state::AppState;
use crate::{commands, panel, windows};

const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");

/// Menu items whose state changes while the app runs.
pub struct TrayItems {
    open: MenuItem<Wry>,
    pause: CheckMenuItem<Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let settings = app.state::<AppState>().settings();
    let open = MenuItem::with_id(
        app,
        "open",
        "Open Clipboard History",
        true,
        Some(&settings.shortcut),
    )?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause",
        "Pause Recording",
        true,
        settings.paused,
        None::<&str>,
    )?;
    let prefs = MenuItem::with_id(app, "settings", "Settings...", true, Some("CmdOrCtrl+,"))?;
    let clear = MenuItem::with_id(app, "clear", "Clear History...", true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "About HateClip", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit HateClip", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &clear,
            &PredefinedMenuItem::separator(app)?,
            &prefs,
            &about,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(true)
        .tooltip("HateClip")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => panel::show(app),
            "pause" => toggle_pause(app),
            "settings" => windows::open_settings(app),
            "clear" => confirm_clear(app),
            "about" => show_about(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayItems { open, pause });
    Ok(())
}

fn toggle_pause(app: &AppHandle) {
    let state = app.state::<AppState>();
    let old = state.settings();
    let mut new = old.clone();
    new.paused = !old.paused;
    if let Err(e) = commands::persist_settings(app, &state, &old, new) {
        tracing::error!("could not toggle pause: {e}");
    }
}

pub fn set_paused(app: &AppHandle, paused: bool) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.pause.set_checked(paused);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let tip = if paused {
            "HateClip (paused)"
        } else {
            "HateClip"
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

pub fn set_shortcut_label(app: &AppHandle, shortcut: &str) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.open.set_accelerator(Some(shortcut));
    }
}

fn confirm_clear(app: &AppHandle) {
    let handle = app.clone();
    app.dialog()
        .message("This removes every item from your clipboard history except pinned items. It cannot be undone.")
        .title("Clear clipboard history?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Clear History".into(), "Cancel".into()))
        .show(move |confirmed| {
            if confirmed {
                let state = handle.state::<AppState>();
                match state.store().clear(false) {
                    Ok(n) => tracing::info!("cleared {n} items"),
                    Err(e) => tracing::error!("clear failed: {e:#}"),
                }
                let _ = tauri::Emitter::emit(&handle, crate::watcher::HISTORY_CHANGED, ());
            }
        });
}

fn show_about(app: &AppHandle) {
    let version = app.package_info().version.to_string();
    app.dialog()
        .message(format!(
            "Version {version}\n\nClipboard history for macOS, written in Rust."
        ))
        .title("HateClip")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}
