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
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

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

/// CG virtual keycode for the combo keys we support evolving into.
fn code_to_keycode(code: Code) -> Option<u16> {
    use Code::*;
    Some(match code {
        Space => 49,
        Enter => 36,
        Tab => 48,
        KeyA => 0,
        KeyS => 1,
        KeyD => 2,
        KeyF => 3,
        KeyH => 4,
        KeyG => 5,
        KeyZ => 6,
        KeyX => 7,
        KeyC => 8,
        KeyV => 9,
        KeyB => 11,
        KeyQ => 12,
        KeyW => 13,
        KeyE => 14,
        KeyR => 15,
        KeyY => 16,
        KeyT => 17,
        Digit1 => 18,
        Digit2 => 19,
        Digit3 => 20,
        Digit4 => 21,
        Digit6 => 22,
        Digit5 => 23,
        Digit9 => 25,
        Digit7 => 26,
        Digit8 => 28,
        Digit0 => 29,
        KeyO => 31,
        KeyU => 32,
        KeyI => 34,
        KeyP => 35,
        KeyL => 37,
        KeyJ => 38,
        KeyK => 40,
        KeyN => 45,
        KeyM => 46,
        F1 => 122,
        F2 => 120,
        F3 => 99,
        F4 => 118,
        F5 => 96,
        F6 => 97,
        F7 => 98,
        F8 => 100,
        F9 => 101,
        F10 => 109,
        F11 => 103,
        F12 => 111,
        ArrowLeft => 123,
        ArrowRight => 124,
        ArrowDown => 125,
        ArrowUp => 126,
        _ => return None,
    })
}

fn mods_to_flags(mods: Modifiers) -> CGEventFlags {
    let mut flags = CGEventFlags::CGEventFlagNull;
    if mods.contains(Modifiers::ALT) {
        flags |= CGEventFlags::CGEventFlagAlternate;
    }
    if mods.contains(Modifiers::SHIFT) {
        flags |= CGEventFlags::CGEventFlagShift;
    }
    if mods.contains(Modifiers::CONTROL) {
        flags |= CGEventFlags::CGEventFlagControl;
    }
    if mods.contains(Modifiers::META) || mods.contains(Modifiers::SUPER) {
        flags |= CGEventFlags::CGEventFlagCommand;
    }
    flags
}

/// Registered combos as (required modifier flags, key). A keydown matching one
/// while a bare modifier is held is a combination GROWING into a profile —
/// never a chord to cancel on.
fn combo_table(profiles: &[Profile]) -> Vec<(CGEventFlags, u16)> {
    profiles
        .iter()
        .filter(|p| parse_bare(&p.hotkey).is_none())
        .filter_map(|p| {
            let shortcut: Shortcut = p.hotkey.parse().ok()?;
            let key = code_to_keycode(shortcut.key)?;
            Some((mods_to_flags(shortcut.mods), key))
        })
        .collect()
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
    /// Toggle press made while recording: the stop fires on a clean release
    /// (a chord like ⌥C mid-recording must not stop-and-paste).
    toggle_stop: bool,
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

    let combos = combo_table(profiles);
    let app = app.clone();
    std::thread::Builder::new()
        .name("speakly-modtap".into())
        .spawn(move || tap_thread(app, engine, map, combos, stop))
        .expect("spawn modifier tap thread");
}

fn tap_thread(
    app: AppHandle,
    engine: Arc<Engine>,
    map: Vec<(u16, String)>,
    combos: Vec<(CGEventFlags, u16)>,
    stop: Arc<AtomicBool>,
) {
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
                            // Starts on press for zero latency (a chord within
                            // the window cancels); a toggle press made while
                            // recording becomes a pending stop that fires on
                            // clean release.
                            let toggle_stop =
                                crate::input::pressed_defer_toggle_stop(&cb_app, profile_id);
                            *slot = Some(ActivePress {
                                keycode,
                                profile_id: profile_id.clone(),
                                pressed_at: Instant::now(),
                                chord_cancelled: false,
                                toggle_stop,
                            });
                        }
                        (Some(press), false) if press.keycode == keycode => {
                            let cancelled = press.chord_cancelled;
                            let toggle_stop = press.toggle_stop;
                            let bare_id = press.profile_id.clone();
                            *slot = None;
                            if cancelled {
                                return CallbackResult::Keep;
                            }
                            if toggle_stop {
                                crate::input::toggle_stop_release(&cb_app);
                            } else {
                                // Hold stops (deferred); toggle no-ops; a
                                // session retargeted onto a combo is ignored —
                                // the combo's own release owns the stop.
                                crate::input::released(&cb_app, &bare_id);
                            }
                        }
                        _ => {}
                    }
                }
                CGEventType::KeyDown => {
                    let mut slot = cb_active.lock().unwrap();
                    if let Some(press) = slot.as_mut() {
                        if !press.chord_cancelled {
                            let keycode = event
                                .get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
                                as u16;
                            let flags = event.get_flags();
                            let grows_into_combo = combos
                                .iter()
                                .any(|(mods, key)| *key == keycode && flags.contains(*mods));
                            if grows_into_combo {
                                // ⌥ held + Space = the ⌥Space profile: the
                                // combination grew. The plugin fires that
                                // combo's Pressed next, which retargets the
                                // running session. Not a chord — don't cancel.
                            } else if press.toggle_stop {
                                // Pending stop: a chord means "don't stop";
                                // recording continues untouched.
                                press.chord_cancelled = true;
                            } else if press.pressed_at.elapsed() < CHORD_WINDOW {
                                // An immediate unrecognized chord like ⌥C
                                // aborts the young dictation.
                                press.chord_cancelled = true;
                                cb_engine.dictation.cancel();
                            }
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
