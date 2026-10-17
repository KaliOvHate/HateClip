use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use hateclip_macos::ReadResult;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
pub const HISTORY_CHANGED: &str = "history-changed";

/// macOS has no clipboard-changed notification, so poll the pasteboard's
/// change counter, which is a cheap integer read.
pub fn spawn(app: AppHandle) {
    thread::Builder::new()
        .name("clipboard-watcher".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let mut last = hateclip_macos::change_count();
            loop {
                thread::sleep(POLL_INTERVAL);
                hateclip_macos::run_in_autorelease_pool(|| {
                    let current = hateclip_macos::change_count();
                    // While we are writing, look again on the next tick, when the
                    // change count of our own write is known.
                    if current == last || state.writing.load(Ordering::SeqCst) {
                        return;
                    }
                    last = current;
                    poll_once(&app, current);
                });
            }
        })
        .expect("failed to start clipboard watcher");
}

fn poll_once(app: &AppHandle, change_count: isize) {
    let state = app.state::<AppState>();
    if change_count == state.own_change_count.load(Ordering::SeqCst)
        || state.paused.load(Ordering::SeqCst)
    {
        return;
    }
    let settings = state.settings();
    let source = hateclip_macos::frontmost_app();
    if settings.is_excluded(source.as_ref().and_then(|s| s.bundle_id.as_deref())) {
        tracing::debug!("skipping copy from excluded app");
        return;
    }

    let mut capture = match hateclip_macos::read_clipboard(settings.record_images) {
        ReadResult::Content(c) => c,
        ReadResult::Concealed => {
            tracing::debug!("skipping concealed clipboard content");
            return;
        }
        ReadResult::Empty => return,
    };
    if let Some(src) = source.filter(|s| s.pid != hateclip_macos::own_pid()) {
        capture.source_bundle_id = src.bundle_id;
        capture.source_name = src.name;
    }

    let mut store = state.store();
    match store.insert(&capture) {
        Ok(Some(_)) => {
            if let Err(e) = store.prune(settings.max_items, settings.max_age_days) {
                tracing::warn!("prune failed: {e:#}");
            }
            drop(store);
            let _ = app.emit(HISTORY_CHANGED, ());
        }
        Ok(None) => {}
        Err(e) => tracing::error!("could not save clipboard item: {e:#}"),
    }
}
