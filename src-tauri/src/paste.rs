//! Text insertion at the cursor: clipboard write → synthetic ⌘V → clipboard
//! restore. Requires the Accessibility permission for the synthetic keystroke;
//! without it we leave the text on the clipboard and tell the user to paste.

use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use tauri::AppHandle;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

pub enum PasteOutcome {
    Pasted,
    ClipboardOnly,
    Failed(String),
}

/// Must run on the main thread (arboard/NSPasteboard); the caller uses
/// `run_on_main_thread`.
pub fn paste_text(_app: &AppHandle, text: &str, restore_clipboard: bool) -> PasteOutcome {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => return PasteOutcome::Failed(format!("clipboard: {e}")),
    };
    // Only text snapshots are restored in v0; the changeCount-guarded
    // full-flavor restore is planned hardening.
    let previous = restore_clipboard
        .then(|| clipboard.get_text().ok())
        .flatten();

    if let Err(e) = clipboard.set_text(text.to_string()) {
        return PasteOutcome::Failed(format!("clipboard write: {e}"));
    }

    if !accessibility_trusted() {
        return PasteOutcome::ClipboardOnly;
    }

    // Let the pasteboard settle before the target app reads it.
    std::thread::sleep(Duration::from_millis(120));
    if let Err(e) = send_cmd_v() {
        return PasteOutcome::Failed(e);
    }

    if let Some(prev) = previous {
        // Restore off-thread after the target app has consumed the paste.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(prev);
            }
        });
    }
    PasteOutcome::Pasted
}

fn send_cmd_v() -> Result<(), String> {
    const KEY_V: u16 = 9; // kVK_ANSI_V
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| "event source".to_string())?;

    let down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|_| "key down event".to_string())?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);

    std::thread::sleep(Duration::from_millis(12));

    let up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|_| "key up event".to_string())?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);
    Ok(())
}
