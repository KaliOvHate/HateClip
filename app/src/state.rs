use std::sync::atomic::{AtomicBool, AtomicIsize};
use std::sync::{Mutex, MutexGuard};

use hateclip_core::{Settings, Store};
use hateclip_macos::AppInfo;

pub struct AppState {
    pub store: Mutex<Store>,
    pub settings: Mutex<Settings>,
    /// The app that was frontmost before the panel opened; pastes go there.
    pub previous_app: Mutex<Option<AppInfo>>,
    /// Change count produced by our own clipboard write, so the watcher skips it.
    pub own_change_count: AtomicIsize,
    pub paused: AtomicBool,
    /// Set while a paste hands focus back, so the panel's blur handler stays quiet.
    pub pasting: AtomicBool,
    /// Set while we write to the clipboard, so the watcher waits for the final
    /// change count instead of recording our own write.
    pub writing: AtomicBool,
}

impl AppState {
    pub fn new(store: Store, settings: Settings) -> Self {
        Self {
            store: Mutex::new(store),
            paused: AtomicBool::new(settings.paused),
            settings: Mutex::new(settings),
            previous_app: Mutex::new(None),
            own_change_count: AtomicIsize::new(-1),
            pasting: AtomicBool::new(false),
            writing: AtomicBool::new(false),
        }
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings;
    }
}
