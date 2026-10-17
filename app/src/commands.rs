use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use hateclip_core::{ClipItem, ItemDetail, Query, Settings};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::state::AppState;
use crate::watcher::HISTORY_CHANGED;
use crate::{panel, tray, windows};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn list_items(state: State<'_, AppState>, query: Query) -> CmdResult<Vec<ClipItem>> {
    state.store().list(&query).map_err(err)
}

#[tauri::command]
pub fn item_detail(state: State<'_, AppState>, id: i64) -> CmdResult<Option<ItemDetail>> {
    state.store().detail(id).map_err(err)
}

#[derive(Serialize)]
pub struct Source {
    bundle_id: String,
    name: String,
}

#[tauri::command]
pub fn list_sources(state: State<'_, AppState>) -> CmdResult<Vec<Source>> {
    Ok(state
        .store()
        .sources()
        .map_err(err)?
        .into_iter()
        .map(|(bundle_id, name)| Source { bundle_id, name })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasteMode {
    /// Paste with all formats.
    Normal,
    /// Paste text without formatting.
    Plain,
    /// Only put the item on the clipboard.
    Copy,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteOutcome {
    Pasted,
    Copied,
    NeedsPermission,
}

#[tauri::command]
pub fn paste_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    mode: PasteMode,
) -> CmdResult<PasteOutcome> {
    let payload = {
        let store = state.store();
        let payload = store
            .payload(id)
            .map_err(err)?
            .ok_or("This item no longer exists.")?;
        store.touch(id).map_err(err)?;
        payload
    };
    state.writing.store(true, Ordering::SeqCst);
    let written = hateclip_macos::write_clipboard(&payload, mode == PasteMode::Plain);
    if let Ok(change_count) = written {
        state.own_change_count.store(change_count, Ordering::SeqCst);
    }
    state.writing.store(false, Ordering::SeqCst);
    written.map_err(err)?;
    let _ = app.emit(HISTORY_CHANGED, ());

    if mode == PasteMode::Copy {
        panel::dismiss(&app);
        return Ok(PasteOutcome::Copied);
    }

    state.pasting.store(true, Ordering::SeqCst);
    panel::hide(&app);
    if !hateclip_macos::is_accessibility_trusted(false) {
        state.pasting.store(false, Ordering::SeqCst);
        windows::open_onboarding(&app);
        return Ok(PasteOutcome::NeedsPermission);
    }

    let previous = state
        .previous_app
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let handle = app.clone();
    thread::spawn(move || {
        if let Some(prev) = previous {
            let pid = prev.pid;
            let _ = handle.run_on_main_thread(move || {
                hateclip_macos::activate_app(pid);
            });
        }
        // Give the target app a moment to become key before the keystroke.
        thread::sleep(Duration::from_millis(150));
        if let Err(e) = hateclip_macos::send_paste_keystroke() {
            tracing::error!("paste keystroke failed: {e:#}");
        }
        handle
            .state::<AppState>()
            .pasting
            .store(false, Ordering::SeqCst);
    });
    Ok(PasteOutcome::Pasted)
}

#[tauri::command]
pub fn set_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> CmdResult<()> {
    state.store().set_pinned(id, pinned).map_err(err)?;
    let _ = app.emit(HISTORY_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub fn delete_item(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.store().delete(id).map_err(err)?;
    let _ = app.emit(HISTORY_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub fn clear_history(
    app: AppHandle,
    state: State<'_, AppState>,
    include_pinned: bool,
) -> CmdResult<usize> {
    let removed = state.store().clear(include_pinned).map_err(err)?;
    let _ = app.emit(HISTORY_CHANGED, ());
    Ok(removed)
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    panel::dismiss(&app);
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CmdResult<Settings> {
    let new = settings.normalized();
    let old = state.settings();
    persist_settings(&app, &state, &old, new)
}

/// Apply a settings change to the running app, then save it.
pub fn persist_settings(
    app: &AppHandle,
    state: &AppState,
    old: &Settings,
    new: Settings,
) -> CmdResult<Settings> {
    if new.shortcut != old.shortcut {
        apply_shortcut(app, Some(&old.shortcut), &new.shortcut)?;
    }
    if new.launch_at_login != old.launch_at_login {
        let autolaunch = app.autolaunch();
        let result = if new.launch_at_login {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        result.map_err(|e| format!("Could not change launch at login: {e}"))?;
    }
    state.paused.store(new.paused, Ordering::SeqCst);
    tray::set_paused(app, new.paused);

    let store = state.store();
    store.save_settings(&new).map_err(err)?;
    if new.max_items != old.max_items || new.max_age_days != old.max_age_days {
        store.prune(new.max_items, new.max_age_days).map_err(err)?;
        let _ = app.emit(HISTORY_CHANGED, ());
    }
    drop(store);
    state.set_settings(new.clone());
    let _ = app.emit("settings-changed", &new);
    Ok(new)
}

/// Swap the global shortcut, restoring the old one if the new one is rejected.
pub fn apply_shortcut(app: &AppHandle, old: Option<&str>, new: &str) -> CmdResult<()> {
    let gs = app.global_shortcut();
    if let Some(old) = old {
        let _ = gs.unregister(old);
    }
    if let Err(e) = gs.register(new) {
        if let Some(old) = old {
            let _ = gs.register(old);
        }
        return Err(format!("The shortcut {new} could not be used: {e}"));
    }
    tray::set_shortcut_label(app, new);
    Ok(())
}

#[tauri::command]
pub fn accessibility_status() -> bool {
    hateclip_macos::is_accessibility_trusted(false)
}

#[tauri::command]
pub fn request_accessibility() -> bool {
    let trusted = hateclip_macos::is_accessibility_trusted(true);
    if !trusted {
        hateclip_macos::open_accessibility_settings();
    }
    trusted
}

#[tauri::command]
pub fn finish_onboarding(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let old = state.settings();
    if !old.onboarding_done {
        let mut new = old.clone();
        new.onboarding_done = true;
        persist_settings(&app, &state, &old, new)?;
    }
    if let Some(w) = app.get_webview_window(windows::ONBOARDING) {
        let _ = w.close();
    }
    Ok(())
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    panel::hide(&app);
    windows::open_settings(&app);
}

#[derive(Serialize)]
pub struct AppInfo {
    name: String,
    version: String,
    data_dir: String,
    item_count: i64,
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    let pkg = app.package_info();
    AppInfo {
        name: pkg.name.clone(),
        version: pkg.version.to_string(),
        data_dir: app
            .path()
            .app_data_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        item_count: state.store().count().unwrap_or(0),
    }
}
