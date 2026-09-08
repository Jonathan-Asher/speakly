//! Bare-modifier hold hotkeys (e.g. hold Right-Option to dictate). The
//! global-shortcut plugin cannot express a lone modifier, so profiles whose
//! hotkey is a bare-modifier spec route through a listen-only CGEventTap on
//! `flagsChanged`. Requires the Accessibility permission (same grant as
//! auto-paste).
//!
//! Chord handling: dictation starts immediately on modifier-down (no latency,
//! no lost speech onset); if a real key follows within the chord window the
//! press was a shortcut like ⌥C — the dictation is cancelled.

use std::collections::HashSet;
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
/// kVK_Escape — cancels a running dictation from anywhere.
const KEYCODE_ESCAPE: u16 = 53;

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

/// A SIDE-SPECIFIC combination, e.g. "RightOption+Space" or "RightOption+KeyM".
/// Carbon global hotkeys cannot tell left from right, so these are owned by the
/// event tap instead of the plugin. Returns (modifier keycode, key keycode).
pub fn parse_sided(hotkey: &str) -> Option<(u16, u16)> {
    let (modifier, key) = hotkey.split_once('+')?;
    let mod_code = parse_bare(modifier)?;
    let key_code = key.parse::<Code>().ok().and_then(keycode_of)?;
    Some((mod_code, key_code))
}

/// Device-DEPENDENT flag bit for one physical modifier key. The named
/// `CGEventFlags` are device-independent (`CGEventFlagAlternate` is set by
/// either Option key), which is exactly why a left/right distinction needs
/// these raw NX bits.
fn device_bit(keycode: u16) -> Option<u64> {
    Some(match keycode {
        61 => 0x0000_0040, // NX_DEVICERALTKEYMASK
        58 => 0x0000_0020, // NX_DEVICELALTKEYMASK
        54 => 0x0000_0010, // NX_DEVICERCMDKEYMASK
        55 => 0x0000_0008, // NX_DEVICELCMDKEYMASK
        _ => return None,
    })
}

/// Is this specific physical modifier down in these flags? Falls back to the
/// device-independent flag for keys without a device bit (Fn).
fn modifier_is_down(keycode: u16, flags: CGEventFlags) -> bool {
    match device_bit(keycode) {
        Some(bit) => flags.bits() & bit != 0,
        // Fn has no device-dependent bit; nothing else reaches here.
        None if keycode == 63 => flags.contains(CGEventFlags::CGEventFlagSecondaryFn),
        None => false,
    }
}

/// CG virtual keycode for the combo keys we support evolving into.
pub fn keycode_of(code: Code) -> Option<u16> {
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
fn combo_table(profiles: &[Profile]) -> Vec<(CGEventFlags, u16, String)> {
    profiles
        .iter()
        .filter(|p| parse_bare(&p.hotkey).is_none() && parse_sided(&p.hotkey).is_none())
        .filter_map(|p| {
            let shortcut: Shortcut = p.hotkey.parse().ok()?;
            let key = keycode_of(shortcut.key)?;
            Some((mods_to_flags(shortcut.mods), key, p.id.clone()))
        })
        .collect()
}

/// Side-specific combinations as (modifier keycode, key keycode, profile id).
fn sided_table(profiles: &[Profile]) -> Vec<(u16, u16, String)> {
    profiles
        .iter()
        .filter_map(|p| parse_sided(&p.hotkey).map(|(m, k)| (m, k, p.id.clone())))
        .collect()
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
    /// Key whose down-event we suppressed (combination growth); its up-event
    /// is suppressed too so other apps never see half a chord.
    swallowed_key: Option<u16>,
    /// Profile the session was retargeted onto by a combination. The plugin
    /// used to deliver that combo's Released; now that the tap swallows those
    /// events, the tap owns the stop — without this the recording never ends.
    retargeted_to: Option<String>,
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
    // The tap serves bare-modifier hotkeys AND Esc-to-cancel, so it is worth
    // running whenever Accessibility allows it — not only for bare profiles.
    let sided = sided_table(profiles);
    let trusted = crate::paste::accessibility_trusted();
    if map.is_empty() && sided.is_empty() && !trusted {
        return;
    }

    if (!map.is_empty() || !sided.is_empty()) && !trusted {
        let _ = app.emit(
            "engine://warning",
            json!({
                "code": "bare_hotkey",
                "message": "Side-specific hotkeys (Right ⌥ and combinations with it) need the Accessibility permission",
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
        .spawn(move || tap_thread(app, engine, map, combos, sided, stop))
        .expect("spawn modifier tap thread");
}

fn tap_thread(
    app: AppHandle,
    engine: Arc<Engine>,
    map: Vec<(u16, String)>,
    combos: Vec<(CGEventFlags, u16, String)>,
    sided: Vec<(u16, u16, String)>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        let restart = run_tap(&app, &engine, &map, &combos, &sided, &stop);
        if !restart {
            return;
        }
        tracing::warn!("event tap was disabled by the system — restarting it");
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// One tap lifetime. Returns true when it should be rebuilt.
#[allow(clippy::too_many_arguments)]
fn run_tap(
    app: &AppHandle,
    engine: &Arc<Engine>,
    map: &[(u16, String)],
    combos: &[(CGEventFlags, u16, String)],
    sided: &[(u16, u16, String)],
    stop: &Arc<AtomicBool>,
) -> bool {
    let app = app.clone();
    let engine = Arc::clone(engine);
    let map = map.to_vec();
    let combos = combos.to_vec();
    let sided = sided.to_vec();
    let stop = Arc::clone(stop);
    // Physical modifiers currently down. Tracked independently of ActivePress
    // so a side-specific combination works even with no bare profile bound.
    let held: Arc<Mutex<HashSet<u16>>> = Arc::new(Mutex::new(HashSet::new()));
    let cb_held = Arc::clone(&held);
    let disabled = Arc::new(AtomicBool::new(false));
    let cb_disabled = Arc::clone(&disabled);
    let active: Arc<Mutex<Option<ActivePress>>> = Arc::new(Mutex::new(None));
    let cb_active = Arc::clone(&active);
    let cb_engine = Arc::clone(&engine);
    let cb_app = app.clone();

    let result = CGEventTap::with_enabled(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        // An ACTIVE tap (not listen-only) so a keystroke that completes one of
        // our combinations can be swallowed before other apps' global hotkeys
        // see it. Nothing is ever dropped outside that narrow case.
        CGEventTapOptions::Default,
        vec![
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::KeyUp,
        ],
        move |_proxy, etype, event| {
            match etype {
                CGEventType::FlagsChanged => {
                    let keycode =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                    // Device-dependent bit: distinguishes left from right, and
                    // keeps the two Option keys from confusing each other.
                    let down = modifier_is_down(keycode, event.get_flags());
                    {
                        let mut h = cb_held.lock().unwrap();
                        if down {
                            h.insert(keycode);
                        } else {
                            h.remove(&keycode);
                        }
                    }
                    let Some((_, profile_id)) = map.iter().find(|(c, _)| *c == keycode) else {
                        return CallbackResult::Keep;
                    };
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
                                swallowed_key: None,
                                retargeted_to: None,
                            });
                        }
                        (Some(press), false) if press.keycode == keycode => {
                            let cancelled = press.chord_cancelled;
                            let toggle_stop = press.toggle_stop;
                            let bare_id = press
                                .retargeted_to
                                .clone()
                                .unwrap_or_else(|| press.profile_id.clone());
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
                    let code =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                    if code == KEYCODE_ESCAPE {
                        // Passed through: the focused app still gets its Esc,
                        // we just also drop the recording.
                        if cb_engine.dictation.is_active() {
                            tracing::info!("escape (tap) — cancelling dictation");
                            *cb_active.lock().unwrap() = None;
                            crate::input::escape(&cb_app);
                        }
                        return CallbackResult::Keep;
                    }
                    // A side-specific combination (RightOption+Space): matched on
                    // the exact physical modifier, so Left-⌥ never triggers it.
                    let sided_hit = {
                        let h = cb_held.lock().unwrap();
                        sided
                            .iter()
                            .find(|(m, k, _)| *k == code && h.contains(m))
                            .map(|(_, _, id)| id.clone())
                    };
                    if let Some(profile_id) = sided_hit {
                        {
                            let mut slot = cb_active.lock().unwrap();
                            if let Some(press) = slot.as_mut() {
                                press.swallowed_key = Some(code);
                                press.retargeted_to = Some(profile_id.clone());
                            }
                        }
                        tracing::info!("side-specific hotkey — {profile_id}");
                        crate::input::pressed(&cb_app, &profile_id);
                        // Swallowed: neither the letter nor another app's hotkey
                        // for the same chord (ChatGPT's ⌥Space) sees this.
                        return CallbackResult::Drop;
                    }

                    let flags = event.get_flags();
                    let grown = combos
                        .iter()
                        .find(|(mods, key, _)| *key == code && flags.contains(*mods))
                        .map(|(_, _, id)| id.clone());

                    let mut slot = cb_active.lock().unwrap();
                    let Some(press) = slot.as_mut() else {
                        return CallbackResult::Keep;
                    };
                    if press.chord_cancelled {
                        return CallbackResult::Keep;
                    }
                    if let Some(profile_id) = grown {
                        // The held combination grew (⌥ + Space = the ⌥Space
                        // profile). Retarget here and SWALLOW the keystroke —
                        // otherwise whatever else claims that combination
                        // system-wide (ChatGPT's ⌥Space, Spotlight, …) fires in
                        // the middle of a dictation.
                        if !cb_engine.dictation.is_active() {
                            return CallbackResult::Keep;
                        }
                        press.swallowed_key = Some(code);
                        press.retargeted_to = Some(profile_id.clone());
                        drop(slot);
                        tracing::info!("combination grew — retargeting to {profile_id}");
                        crate::input::pressed(&cb_app, &profile_id);
                        return CallbackResult::Drop;
                    }
                    if press.toggle_stop {
                        // Pending stop: a chord means "don't stop"; recording
                        // continues untouched.
                        press.chord_cancelled = true;
                    } else if press.pressed_at.elapsed() < CHORD_WINDOW {
                        // An immediate unrecognized chord like ⌥C aborts the
                        // young dictation.
                        press.chord_cancelled = true;
                        cb_engine.dictation.cancel();
                    }
                }
                CGEventType::KeyUp => {
                    let code =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                    let mut slot = cb_active.lock().unwrap();
                    if let Some(press) = slot.as_mut() {
                        if press.swallowed_key == Some(code) {
                            press.swallowed_key = None;
                            return CallbackResult::Drop;
                        }
                    }
                }
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                    // macOS switched us off (a stalled callback, or a security
                    // event). Signal the pump so the tap is rebuilt — otherwise
                    // the hotkeys silently stop working until the next restart.
                    cb_disabled.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
            CallbackResult::Keep
        },
        || {
            while !stop.load(Ordering::Relaxed) && !disabled.load(Ordering::Relaxed) {
                CFRunLoop::run_in_mode(
                    unsafe { kCFRunLoopDefaultMode },
                    Duration::from_millis(200),
                    false,
                );
            }
        },
    );

    if result.is_ok() {
        return disabled.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed);
    }
    {
        tracing::warn!("modifier tap install failed (Accessibility missing?)");
        let _ = app.emit(
            "engine://warning",
            json!({
                "code": "bare_hotkey",
                "message": "Could not listen for the modifier hotkey — grant Accessibility and try again",
            }),
        );
    }
    // Install failed (no Accessibility): retrying in a loop would spin.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_side_specific_combinations() {
        assert_eq!(parse_sided("RightOption+Space"), Some((61, 49)));
        assert_eq!(parse_sided("RightOption+KeyM"), Some((61, 46)));
        assert_eq!(parse_sided("LeftOption+KeyB"), Some((58, 11)));
        assert_eq!(parse_sided("RightCommand+KeyE"), Some((54, 14)));
    }

    #[test]
    fn rejects_plain_and_malformed_specs() {
        // Plain accelerators stay on the plugin path.
        assert_eq!(parse_sided("Alt+Space"), None);
        assert_eq!(parse_sided("Shift+Alt+Space"), None);
        // Bare specs are not combinations.
        assert_eq!(parse_sided("RightOption"), None);
        assert_eq!(parse_sided("RightOption+"), None);
        assert_eq!(parse_sided("RightOption+Nonsense"), None);
        assert_eq!(parse_sided("Nonsense+KeyM"), None);
    }

    #[test]
    fn device_bits_tell_the_two_option_keys_apart() {
        let right = CGEventFlags::from_bits_retain(0x0008_0040); // Alternate + right
        let left = CGEventFlags::from_bits_retain(0x0008_0020); // Alternate + left
        assert!(modifier_is_down(61, right));
        assert!(!modifier_is_down(58, right));
        assert!(modifier_is_down(58, left));
        assert!(!modifier_is_down(61, left));
        // Releasing one while the other is held must read as up for that key.
        assert!(!modifier_is_down(
            61,
            CGEventFlags::from_bits_retain(0x0008_0020)
        ));
    }

    #[test]
    fn tables_route_each_spec_to_exactly_one_owner() {
        let profile = |id: &str, hotkey: &str| Profile {
            id: id.into(),
            name: id.into(),
            hotkey: hotkey.into(),
            mode: speakly_engine_types::DictationMode::Hold,
            language: "he".into(),
            model_id: "he-turbo".into(),
            translate: None,
            auto_paste: true,
            restore_clipboard: true,
        };
        let profiles = vec![
            profile("he", "RightOption"),
            profile("he-en", "RightOption+Space"),
            profile("plain", "Alt+Space"),
        ];
        let sided = sided_table(&profiles);
        assert_eq!(sided, vec![(61, 49, "he-en".to_string())]);
        // The plain accelerator is the only combo-table entry; the sided one
        // must not also be registered there.
        let combos = combo_table(&profiles);
        assert_eq!(combos.len(), 1);
        assert_eq!(combos[0].2, "plain");
    }
}
