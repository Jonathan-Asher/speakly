//! Bare-modifier hold hotkeys (e.g. hold Right-Option to dictate). The
//! global-shortcut plugin cannot express a lone modifier, so profiles whose
//! hotkey is a bare-modifier spec route through a listen-only CGEventTap on
//! `flagsChanged`. Requires the Accessibility permission (same grant as
//! auto-paste).
//!
//! Chord handling: dictation starts immediately on modifier-down (no latency,
//! no lost speech onset); if a real key follows within the chord window the
//! press was a shortcut like ⌥C — the dictation is cancelled.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use serde_json::json;
use speakly_engine::Engine;
use speakly_engine_types::Profile;
use tauri::{AppHandle, Emitter, Manager};

const CHORD_WINDOW: Duration = Duration::from_millis(150);

/// Supported bare-modifier hotkey specs → macOS virtual keycode.
const BARE_SPECS: &[(&str, u16)] = &[
    ("RightOption", 61),
    ("LeftOption", 58),
    ("RightCommand", 54),
    ("Fn", 63),
];

pub fn parse_bare(hotkey: &str) -> Option<u16> {
    BARE_SPECS
        .iter()
        .find(|(name, _)| *name == hotkey)
        .map(|(_, code)| *code)
}

fn family_flag(keycode: u16) -> CGEventFlags {
    match keycode {
        61 | 58 => CGEventFlags::CGEventFlagAlternate,
        54 | 55 => CGEventFlags::CGEventFlagCommand,
        63 => CGEventFlags::CGEventFlagSecondaryFn,
        _ => CGEventFlags::CGEventFlagNull,
    }
}

/// Stop flag of the currently running tap thread, if any.
pub struct TapState(pub Mutex<Option<Arc<AtomicBool>>>);

impl Default for TapState {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

struct ActivePress {
    keycode: u16,
    profile_id: String,
    pressed_at: Instant,
    chord_cancelled: bool,
}

/// Replace the running tap (if any) with one covering the given bare-modifier
/// profiles. Call with the full current profile set on every (re)registration;
/// an empty bare set just stops the tap.
pub fn sync(app: &AppHandle, engine: Arc<Engine>, profiles: &[Profile]) {
    let map: Vec<(u16, String)> = profiles
        .iter()
        .filter_map(|p| parse_bare(&p.hotkey).map(|code| (code, p.id.clone())))
        .collect();

    let state = app.state::<TapState>();
    let mut guard = state.0.lock().unwrap();
    if let Some(stop) = guard.take() {
        stop.store(true, Ordering::Relaxed);
    }
    if map.is_empty() {
        return;
    }

    if !crate::paste::accessibility_trusted() {
        let _ = app.emit(
            "engine://warning",
            json!({
                "code": "bare_hotkey",
                "message": "Modifier-hold hotkeys need the Accessibility permission",
            }),
        );
    }

    let stop = Arc::new(AtomicBool::new(false));
    *guard = Some(Arc::clone(&stop));
    drop(guard);

    let app = app.clone();
    std::thread::Builder::new()
        .name("speakly-modtap".into())
        .spawn(move || tap_thread(app, engine, map, stop))
        .expect("spawn modifier tap thread");
}

fn tap_thread(app: AppHandle, engine: Arc<Engine>, map: Vec<(u16, String)>, stop: Arc<AtomicBool>) {
    let active: Arc<Mutex<Option<ActivePress>>> = Arc::new(Mutex::new(None));
    let cb_active = Arc::clone(&active);
    let cb_engine = Arc::clone(&engine);
    let cb_app = app.clone();

    let result = CGEventTap::with_enabled(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
        move |_proxy, etype, event| {
            match etype {
                CGEventType::FlagsChanged => {
                    let keycode =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                    let Some((_, profile_id)) = map.iter().find(|(c, _)| *c == keycode) else {
                        return CallbackResult::Keep;
                    };
                    let down = event.get_flags().contains(family_flag(keycode));
                    let mut slot = cb_active.lock().unwrap();
                    match (&*slot, down) {
                        (None, true) => {
                            *slot = Some(ActivePress {
                                keycode,
                                profile_id: profile_id.clone(),
                                pressed_at: Instant::now(),
                                chord_cancelled: false,
                            });
                            crate::shortcuts::start_profile(&cb_app, &cb_engine, profile_id);
                        }
                        (Some(press), false) if press.keycode == keycode => {
                            let cancelled = press.chord_cancelled;
                            let _ = &press.profile_id;
                            *slot = None;
                            if !cancelled {
                                cb_engine.dictation.stop();
                            }
                        }
                        _ => {}
                    }
                }
                CGEventType::KeyDown => {
                    let mut slot = cb_active.lock().unwrap();
                    if let Some(press) = slot.as_mut() {
                        if !press.chord_cancelled && press.pressed_at.elapsed() < CHORD_WINDOW {
                            // Modifier + key within the window = a chord like
                            // ⌥C, not push-to-talk. Abort quietly.
                            press.chord_cancelled = true;
                            cb_engine.dictation.cancel();
                        }
                    }
                }
                _ => {}
            }
            CallbackResult::Keep
        },
        || {
            while !stop.load(Ordering::Relaxed) {
                CFRunLoop::run_in_mode(
                    unsafe { kCFRunLoopDefaultMode },
                    Duration::from_millis(200),
                    false,
                );
            }
        },
    );

    if result.is_err() {
        tracing::warn!("modifier tap install failed (Accessibility missing?)");
        let _ = app.emit(
            "engine://warning",
            json!({
                "code": "bare_hotkey",
                "message": "Could not listen for the modifier hotkey — grant Accessibility and try again",
            }),
        );
    }
}
