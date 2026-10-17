use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const SETTINGS: &str = "settings";
pub const ONBOARDING: &str = "onboarding";

fn open(app: &AppHandle, label: &str, url: &str, title: &str, size: (f64, f64)) {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        return;
    }
    match WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(size.0, size.1)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .center()
        .focused(true)
        .build()
    {
        Ok(win) => {
            let _ = win.set_focus();
        }
        Err(e) => tracing::error!("could not open {label} window: {e}"),
    }
}

pub fn open_settings(app: &AppHandle) {
    open(
        app,
        SETTINGS,
        "settings.html",
        "HateClip Settings",
        (580.0, 620.0),
    );
}

pub fn open_onboarding(app: &AppHandle) {
    open(
        app,
        ONBOARDING,
        "onboarding.html",
        "Welcome to HateClip",
        (520.0, 560.0),
    );
}
