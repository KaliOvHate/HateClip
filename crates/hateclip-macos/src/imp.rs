#![allow(unused_unsafe)]

use std::ffi::c_void;
use std::process::Command;

use anyhow::{anyhow, Result};
use hateclip_core::model::uti;
use hateclip_core::{Capture, Payload, Representation};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSPasteboard, NSPasteboardItem, NSPasteboardWriting,
    NSRunningApplication, NSWindow, NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};

use crate::{AppInfo, ReadResult, ACCESSIBILITY_SETTINGS_URL, PRIVATE_TYPES};

/// Virtual key code of the V key on ANSI and ISO layouts.
const KEY_V: u16 = 9;
/// kCGStatusWindowLevel: above normal and floating windows.
const PANEL_WINDOW_LEVEL: isize = 25;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

fn general() -> Retained<NSPasteboard> {
    unsafe { NSPasteboard::generalPasteboard() }
}

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

pub fn run_in_autorelease_pool<T>(f: impl FnOnce() -> T) -> T {
    autoreleasepool(|_| f())
}

pub fn change_count() -> isize {
    unsafe { general().changeCount() }
}

pub fn read_clipboard(include_images: bool) -> ReadResult {
    let pb = general();
    let types: Vec<String> = match unsafe { pb.types() } {
        Some(t) => t.iter().map(|s| s.to_string()).collect(),
        None => return ReadResult::Empty,
    };
    if types.iter().any(|t| PRIVATE_TYPES.contains(&t.as_str())) {
        return ReadResult::Concealed;
    }
    let has = |t: &str| types.iter().any(|x| x == t);

    let mut cap = Capture::default();

    if has(uti::FILE_URL) {
        if let Some(items) = unsafe { pb.pasteboardItems() } {
            for item in items.iter() {
                let Some(url) = (unsafe { item.stringForType(&ns(uti::FILE_URL)) }) else {
                    continue;
                };
                if let Some(path) =
                    unsafe { NSURL::URLWithString(&url) }.and_then(|u| unsafe { u.path() })
                {
                    cap.files.push(path.to_string());
                }
            }
        }
    }

    if has(uti::PLAIN_TEXT) {
        cap.text = unsafe { pb.stringForType(&ns(uti::PLAIN_TEXT)) }.map(|s| s.to_string());
        for rich in [uti::RTF, uti::HTML] {
            if has(rich) {
                if let Some(data) = unsafe { pb.dataForType(&ns(rich)) } {
                    cap.rich.push(Representation {
                        uti: rich.into(),
                        data: data.to_vec(),
                    });
                }
            }
        }
    }

    if include_images && cap.files.is_empty() {
        for img in [uti::PNG, uti::TIFF] {
            if has(img) {
                if let Some(data) = unsafe { pb.dataForType(&ns(img)) } {
                    cap.image = Some(Representation {
                        uti: img.into(),
                        data: data.to_vec(),
                    });
                    break;
                }
            }
        }
    }

    if cap.kind().is_some() {
        ReadResult::Content(cap)
    } else {
        ReadResult::Empty
    }
}

/// Put an item on the clipboard. Returns the new change count so the watcher
/// can skip our own write.
pub fn write_clipboard(payload: &Payload, plain_text_only: bool) -> Result<isize> {
    let pb = general();
    // Work out what to write before clearing, so a failure leaves the clipboard alone.
    let plain = if plain_text_only {
        let text = payload
            .text
            .clone()
            .or_else(|| (!payload.files.is_empty()).then(|| payload.files.join("\n")))
            .ok_or_else(|| anyhow!("This item has no text to paste as plain text."))?;
        Some(text)
    } else {
        None
    };
    unsafe { pb.clearContents() };

    if let Some(text) = plain {
        unsafe { pb.setString_forType(&ns(&text), &ns(uti::PLAIN_TEXT)) };
        return Ok(change_count());
    }

    if !payload.files.is_empty() {
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = payload
            .files
            .iter()
            .filter_map(|path| {
                let url = unsafe { NSURL::fileURLWithPath(&ns(path)) };
                let url_str = unsafe { url.absoluteString() }?;
                let item = unsafe { NSPasteboardItem::new() };
                unsafe { item.setString_forType(&url_str, &ns(uti::FILE_URL)) };
                unsafe { item.setString_forType(&ns(path), &ns(uti::PLAIN_TEXT)) };
                Some(ProtocolObject::from_retained(item))
            })
            .collect();
        let array = NSArray::from_retained_slice(&items);
        if !unsafe { pb.writeObjects(&array) } {
            return Err(anyhow!("could not write files to the clipboard"));
        }
        return Ok(change_count());
    }

    if let Some(text) = &payload.text {
        unsafe { pb.setString_forType(&ns(text), &ns(uti::PLAIN_TEXT)) };
    }
    for rep in &payload.rich {
        let data = NSData::with_bytes(&rep.data);
        unsafe { pb.setData_forType(Some(&data), &ns(&rep.uti)) };
    }
    if let Some(img) = &payload.image {
        let data = NSData::with_bytes(&img.data);
        unsafe { pb.setData_forType(Some(&data), &ns(&img.uti)) };
    }
    Ok(change_count())
}

pub fn frontmost_app() -> Option<AppInfo> {
    let ws = unsafe { NSWorkspace::sharedWorkspace() };
    let app = unsafe { ws.frontmostApplication() }?;
    Some(AppInfo {
        bundle_id: unsafe { app.bundleIdentifier() }.map(|s| s.to_string()),
        name: unsafe { app.localizedName() }.map(|s| s.to_string()),
        pid: unsafe { app.processIdentifier() },
    })
}

pub fn own_pid() -> i32 {
    std::process::id() as i32
}

pub fn activate_app(pid: i32) -> bool {
    match unsafe { NSRunningApplication::runningApplicationWithProcessIdentifier(pid) } {
        Some(app) => unsafe {
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows)
        },
        None => false,
    }
}

/// Whether we may post keystrokes to other apps. With `prompt`, macOS shows its
/// own dialog pointing the user to System Settings.
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let dict =
        CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::from(prompt).as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) != 0 }
}

pub fn open_accessibility_settings() {
    let _ = Command::new("open").arg(ACCESSIBILITY_SETTINGS_URL).spawn();
}

/// Post Cmd+V to the frontmost app.
pub fn send_paste_keystroke() -> Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow!("could not create event source"))?;
    for key_down in [true, false] {
        let event = CGEvent::new_keyboard_event(source.clone(), KEY_V, key_down)
            .map_err(|_| anyhow!("could not create key event"))?;
        event.set_flags(CGEventFlags::CGEventFlagCommand);
        event.post(CGEventTapLocation::HID);
    }
    Ok(())
}

/// Make the history panel follow the user across Spaces and appear above
/// full-screen apps. Must be called on the main thread.
///
/// # Safety
/// `ns_window` must be a valid pointer to an `NSWindow`.
pub unsafe fn configure_panel_window(ns_window: *mut c_void) {
    let window: &NSWindow = unsafe { &*(ns_window as *const NSWindow) };
    unsafe {
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setLevel(PANEL_WINDOW_LEVEL);
    }
}
