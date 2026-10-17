// HateClip: clipboard history for macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod panel;
mod state;
mod tray;
mod watcher;
mod windows;

use std::sync::atomic::Ordering;

use anyhow::Context;
use hateclip_core::Store;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::state::AppState;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            panel::show(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        panel::toggle(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::item_detail,
            commands::list_sources,
            commands::paste_item,
            commands::set_pinned,
            commands::delete_item,
            commands::clear_history,
            commands::hide_panel,
            commands::get_settings,
            commands::save_settings,
            commands::accessibility_status,
            commands::request_accessibility,
            commands::finish_onboarding,
            commands::open_settings,
            commands::app_info,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            init_logging(app.handle());

            let data_dir = app.path().app_data_dir().context("no app data directory")?;
            let store = Store::open(&data_dir)?;
            let settings = store.load_settings()?;
            tracing::info!(dir = %data_dir.display(), items = store.count()?, "store opened");
            let show_onboarding = !settings.onboarding_done;
            app.manage(AppState::new(store, settings.clone()));

            commands::apply_shortcut(app.handle(), None, &settings.shortcut)
                .unwrap_or_else(|e| tracing::error!("shortcut registration failed: {e}"));
            tray::create(app.handle())?;
            panel::configure(app.handle());
            watcher::spawn(app.handle().clone());

            if show_onboarding {
                windows::open_onboarding(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != panel::LABEL {
                return;
            }
            match event {
                // Clicking anywhere else closes the panel, like a menu.
                WindowEvent::Focused(false) => {
                    let state = window.state::<AppState>();
                    if !state.pasting.load(Ordering::SeqCst) {
                        let _ = window.hide();
                    }
                }
                // The panel is created once and reused; Cmd+W must only hide it.
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building HateClip")
        .run(|_app, event| {
            // Closing the settings window must not quit a menu bar app; only the
            // Quit menu item (which passes an exit code) does.
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}

fn init_logging(app: &tauri::AppHandle) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = app.path().app_log_dir().ok().map(|dir| {
        let appender = tracing_appender::rolling::daily(dir, "hateclip.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        // The guard flushes on drop; the app logs until the process exits.
        std::mem::forget(guard);
        fmt::layer().with_ansi(false).with_writer(writer)
    });
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .try_init();
}
