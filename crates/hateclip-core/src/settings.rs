use serde::{Deserialize, Serialize};

/// User preferences, stored as JSON in the settings table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Global shortcut that opens the history panel, in Tauri accelerator syntax.
    pub shortcut: String,
    /// Maximum number of unpinned items kept.
    pub max_items: u32,
    /// Unpinned items older than this are removed. 0 keeps them forever.
    pub max_age_days: u32,
    /// Bundle identifiers of apps whose copies are never recorded.
    pub excluded_apps: Vec<String>,
    pub record_images: bool,
    pub launch_at_login: bool,
    pub paused: bool,
    pub onboarding_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "CmdOrCtrl+Shift+V".into(),
            max_items: 1000,
            max_age_days: 0,
            excluded_apps: vec![
                "com.apple.keychainaccess".into(),
                "com.apple.Passwords".into(),
                "com.1password.1password".into(),
                "com.agilebits.onepassword7".into(),
                "com.bitwarden.desktop".into(),
            ],
            record_images: true,
            launch_at_login: false,
            paused: false,
            onboarding_done: false,
        }
    }
}

impl Settings {
    pub fn is_excluded(&self, bundle_id: Option<&str>) -> bool {
        bundle_id.is_some_and(|id| {
            self.excluded_apps
                .iter()
                .any(|e| e.eq_ignore_ascii_case(id))
        })
    }

    /// Clamp values a user could set out of range.
    pub fn normalized(mut self) -> Self {
        self.max_items = self.max_items.clamp(10, 100_000);
        self.max_age_days = self.max_age_days.min(3650);
        self.excluded_apps.retain(|s| !s.trim().is_empty());
        if self.shortcut.trim().is_empty() {
            self.shortcut = Settings::default().shortcut;
        }
        self
    }
}
