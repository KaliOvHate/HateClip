use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

use crate::state::AppState;

pub const LABEL: &str = "panel";

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// One-time native setup: join all Spaces and float above full-screen apps.
pub fn configure(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Some(win) = window(app) {
        if let Ok(ns_window) = win.ns_window() {
            // SAFETY: Tauri returns the live NSWindow; setup runs on the main thread.
            unsafe { hateclip_macos::configure_panel_window(ns_window) };
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn toggle(app: &AppHandle) {
    match window(app) {
        Some(w) if w.is_visible().unwrap_or(false) => hide(app),
        _ => show(app),
    }
}

pub fn show(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    let state = app.state::<AppState>();
    // Remember where the user was so a paste or Esc can return there. When one of
    // our own windows was in front, there is nothing to switch back to.
    let front = hateclip_macos::frontmost_app().filter(|f| f.pid != hateclip_macos::own_pid());
    *state.previous_app.lock().unwrap_or_else(|e| e.into_inner()) = front;
    position_on_cursor_screen(app, &win);
    let _ = win.show();
    let _ = win.set_focus();
    let _ = win.emit("panel-shown", ());
}

pub fn hide(app: &AppHandle) {
    if let Some(win) = window(app) {
        let _ = win.hide();
    }
}

/// Close the panel and hand focus back to the app the user was working in.
/// Hiding alone would leave HateClip active with no window, so keystrokes would
/// go nowhere until the user clicked somewhere.
pub fn dismiss(app: &AppHandle) {
    hide(app);
    let previous = app
        .state::<AppState>()
        .previous_app
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(prev) = previous {
        hateclip_macos::activate_app(prev.pid);
    }
}

/// Center the panel slightly above the middle of the screen the mouse is on.
fn position_on_cursor_screen(app: &AppHandle, win: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, win.outer_size()) else {
        let _ = win.center();
        return;
    };
    let area = monitor.work_area();
    let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
    let y = area.position.y + (area.size.height as i32 - size.height as i32) / 3;
    let _ = win.set_position(PhysicalPosition::new(x, y.max(area.position.y)));
}
