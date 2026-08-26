//! Central dictation-input dispatcher. Every key source (global-shortcut
//! plugin, bare-modifier event tap) reports here; this module owns the
//! session-level interaction rules:
//!
//! - start on key-down for both modes (zero latency);
//! - a growing combination RETARGETS the running session (⌥ held → +Space =
//!   ⌥Space: same recording, now the ⌥Space profile — the final combination
//!   wins);
//! - a shrinking combination stops a hold session, with a short deferral so a
//!   grow racing its release event can't cause a spurious stop-and-paste;
//! - toggle stops on a clean second tap;
//! - Esc cancels the session.
//!
//! Nothing here ever calls back into the global-shortcut plugin from inside
//! its own handler — (un)registering Esc is deferred to the main thread from a
//! detached thread (re-entering the plugin deadlocks it).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use speakly_engine::Engine;
use speakly_engine_types::DictationMode;
use tauri::{AppHandle, Manager};

/// Grace before a hold-release actually stops: a combination change delivers
/// Released(old) and Pressed(new) in unspecified order.
const STOP_DEFER: Duration = Duration::from_millis(60);

#[derive(Default)]
pub struct InputState {
    /// Cancel token of the pending deferred hold-stop, if any.
    deferred_stop: Mutex<Option<Arc<AtomicBool>>>,
}

fn engine(app: &AppHandle) -> Arc<Engine> {
    Arc::clone(&*app.state::<Arc<Engine>>())
}

fn cancel_deferred_stop(app: &AppHandle) {
    if let Some(token) = app
        .state::<InputState>()
        .deferred_stop
        .lock()
        .unwrap()
        .take()
    {
        token.store(true, Ordering::Relaxed);
    }
}

fn schedule_stop(app: &AppHandle) {
    let token = Arc::new(AtomicBool::new(false));
    *app.state::<InputState>().deferred_stop.lock().unwrap() = Some(Arc::clone(&token));
    let engine = engine(app);
    std::thread::spawn(move || {
        std::thread::sleep(STOP_DEFER);
        if !token.load(Ordering::Relaxed) {
            engine.dictation.stop();
        }
    });
}

/// Mode of a profile, read fresh from settings.
fn mode_of(app: &AppHandle, profile_id: &str) -> DictationMode {
    let state = app.state::<crate::settings::SettingsState>();
    let settings = state.0.lock().unwrap();
    settings
        .profile(profile_id)
        .map(|p| p.mode)
        .unwrap_or(DictationMode::Hold)
}

/// A profile's combination went DOWN (combo Pressed, or bare modifier down).
pub fn pressed(app: &AppHandle, profile_id: &str) {
    cancel_deferred_stop(app);
    let engine = engine(app);
    match engine.dictation.active_profile_id() {
        None => crate::shortcuts::start_profile(app, &engine, profile_id),
        Some(current) if current == profile_id => {
            // Second press of the running profile: toggle stops (on the tap's
            // clean release for bare keys, immediately for combos — the caller
            // decides by routing through `toggle_stop_press`). Hold ignores
            // repeats.
            if mode_of(app, profile_id) == DictationMode::Toggle {
                engine.dictation.stop();
            }
        }
        Some(_) => {
            // The combination evolved into a different profile: retarget the
            // running session — same audio, new language/model/translation.
            crate::shortcuts::retarget_profile(app, &engine, profile_id);
        }
    }
}

/// Like `pressed`, but the toggle-stop must wait for a clean release (bare
/// modifiers: a chord like ⌥C mid-recording must not stop-and-paste). Returns
/// true when the caller should treat this press as a pending stop.
pub fn pressed_defer_toggle_stop(app: &AppHandle, profile_id: &str) -> bool {
    cancel_deferred_stop(app);
    let engine = engine(app);
    match engine.dictation.active_profile_id() {
        None => {
            crate::shortcuts::start_profile(app, &engine, profile_id);
            false
        }
        Some(current) if current == profile_id => mode_of(app, profile_id) == DictationMode::Toggle,
        Some(_) => {
            crate::shortcuts::retarget_profile(app, &engine, profile_id);
            false
        }
    }
}

/// A profile's combination went UP.
pub fn released(app: &AppHandle, profile_id: &str) {
    let engine = engine(app);
    let Some(current) = engine.dictation.active_profile_id() else {
        return;
    };
    if current != profile_id {
        // Session was retargeted elsewhere; the new combination owns the stop.
        return;
    }
    if mode_of(app, profile_id) == DictationMode::Hold {
        schedule_stop(app);
    }
}

/// Confirmed clean release of a pending bare-modifier toggle stop.
pub fn toggle_stop_release(app: &AppHandle) {
    engine(app).dictation.stop();
}

pub fn escape(app: &AppHandle) {
    cancel_deferred_stop(app);
    engine(app).dictation.cancel();
}

/// Arm/disarm the global Esc-cancel hotkey. Always deferred to the main
/// thread from a detached thread so it can never run inside the plugin's own
/// dispatch (that deadlocks) nor block an engine thread.
pub fn set_escape_armed(app: &AppHandle, armed: bool) {
    let app = app.clone();
    std::thread::spawn(move || {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let Ok(esc) = "Escape".parse::<tauri_plugin_global_shortcut::Shortcut>() else {
                return;
            };
            let result = if armed {
                handle.global_shortcut().register(esc)
            } else {
                handle.global_shortcut().unregister(esc)
            };
            if let Err(e) = result {
                tracing::debug!("escape hotkey ({armed}): {e}");
            }
        });
    });
}
