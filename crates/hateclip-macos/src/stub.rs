use std::ffi::c_void;

use anyhow::{bail, Result};
use hateclip_core::Payload;

use crate::{AppInfo, ReadResult};

pub fn change_count() -> isize {
    0
}

pub fn read_clipboard(_include_images: bool) -> ReadResult {
    ReadResult::Empty
}

pub fn write_clipboard(_payload: &Payload, _plain_text_only: bool) -> Result<isize> {
    bail!("clipboard access is only implemented on macOS")
}

pub fn frontmost_app() -> Option<AppInfo> {
    None
}

pub fn own_pid() -> i32 {
    std::process::id() as i32
}

pub fn activate_app(_pid: i32) -> bool {
    false
}

pub fn is_accessibility_trusted(_prompt: bool) -> bool {
    false
}

pub fn open_accessibility_settings() {}

pub fn send_paste_keystroke() -> Result<()> {
    bail!("paste simulation is only implemented on macOS")
}

pub fn run_in_autorelease_pool<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// # Safety
/// `_ns_window` must be a valid NSWindow pointer.
pub unsafe fn configure_panel_window(_ns_window: *mut c_void) {}
