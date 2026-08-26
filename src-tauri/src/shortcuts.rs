//! Global hotkey registration from dictation profiles. Normal combos via the
//! global-shortcut plugin deliver Pressed/Released, so hold-to-talk needs no
//! permissions. Bare-modifier hold (e.g. Right-Option alone) is a later,
//! Accessibility-gated mode.

use std::sync::Arc;

use speakly_engine::{DictationSpec, Engine};
use speakly_engine_types::DictationMode;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::settings::{Settings, SettingsState};

/// Register every profile's hotkey. Returns one human-readable message per
/// profile that could not be registered (bad accelerator, or the OS refused —
/// typically another app holds the combo).
pub fn register_all(app: &AppHandle, engine: Arc<Engine>, settings: &Settings) -> Vec<String> {
    let mut errors = Vec::new();
    for profile in &settings.profiles {
        // Bare-modifier specs (e.g. "RightOption") bypass the plugin and are
        // handled by the flagsChanged event tap, synced below.
        if crate::modifier_tap::parse_bare(&profile.hotkey).is_some() {
            if !crate::paste::accessibility_trusted() {
                errors.push(format!(
                    "{}: '{}' needs the Accessibility permission",
                    profile.name, profile.hotkey
                ));
            }
            continue;
        }
        let shortcut: Shortcut = match profile.hotkey.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "profile {}: bad hotkey '{}': {e:?}",
                    profile.id,
                    profile.hotkey
                );
                errors.push(format!(
                    "{}: hotkey '{}' is not valid",
                    profile.name, profile.hotkey
                ));
                continue;
            }
        };
        // Registration only — dispatch happens in `handle_event`, which reads
        // the profile's CURRENT mode from settings on every event. Per-shortcut
        // closures captured the mode at registration time and could go stale.
        let result = app.global_shortcut().register(shortcut);
        match result {
            Ok(()) => tracing::info!("profile {} on {}", profile.id, profile.hotkey),
            Err(e) => {
                tracing::warn!("register {} failed: {e}", profile.hotkey);
                errors.push(format!(
                    "{}: could not register '{}' ({e})",
                    profile.name, profile.hotkey
                ));
            }
        }
    }
    crate::modifier_tap::sync(app, engine, &settings.profiles);
    errors
}

/// Drop every registered hotkey and re-register from the given settings.
/// Used after profile mutations.
pub fn reregister(app: &AppHandle, engine: Arc<Engine>, settings: &Settings) -> Vec<String> {
    if let Err(e) = app.global_shortcut().unregister_all() {
        tracing::warn!("unregister_all failed: {e}");
    }
    register_all(app, engine, settings)
}

/// The plugin's single global handler: resolve the fired shortcut to a profile
/// at event time (fresh mode, fresh settings) and drive the engine. The
/// settings lock is dropped before any engine call — engine events run
/// synchronously on this thread.
pub fn handle_event(app: &AppHandle, fired: &Shortcut, state: ShortcutState) {
    // Esc is registered only while a dictation is listening (see sink.rs);
    // it cancels — recording is discarded, nothing is pasted.
    if let Ok(esc) = "Escape".parse::<Shortcut>() {
        if *fired == esc {
            if state == ShortcutState::Pressed {
                let engine = Arc::clone(&*app.state::<Arc<Engine>>());
                engine.dictation.cancel();
            }
            return;
        }
    }
    let resolved = {
        let settings_state = app.state::<SettingsState>();
        let settings = settings_state.0.lock().unwrap();
        settings
            .profiles
            .iter()
            .filter(|p| crate::modifier_tap::parse_bare(&p.hotkey).is_none())
            .find(|p| {
                p.hotkey
                    .parse::<Shortcut>()
                    .map(|s| s == *fired)
                    .unwrap_or(false)
            })
            .map(|p| (p.id.clone(), p.mode))
    };
    let Some((profile_id, mode)) = resolved else {
        return;
    };
    let engine = Arc::clone(&*app.state::<Arc<Engine>>());
    match (mode, state) {
        (DictationMode::Hold, ShortcutState::Pressed) => start_profile(app, &engine, &profile_id),
        (DictationMode::Hold, ShortcutState::Released) => engine.dictation.stop(),
        (DictationMode::Toggle, ShortcutState::Pressed) => {
            if engine.dictation.is_active() {
                engine.dictation.stop();
            } else {
                start_profile(app, &engine, &profile_id);
            }
        }
        (DictationMode::Toggle, ShortcutState::Released) => {}
    }
}

pub(crate) fn start_profile(app: &AppHandle, engine: &Engine, profile_id: &str) {
    // Resolve everything and DROP the settings lock before entering the
    // engine: `start` emits events synchronously on this thread, and event
    // handlers (sound cue, paste prefs) read settings — holding the guard
    // across the call self-deadlocks the main thread.
    let spec = {
        let state = app.state::<SettingsState>();
        let settings = state.0.lock().unwrap();
        let Some(profile) = settings.profile(profile_id) else {
            return;
        };
        let Some(model) = settings.models.get(&profile.model_id) else {
            return;
        };
        if model.path.is_empty() {
            tracing::warn!("model {} has no file; skipping", profile.model_id);
            return;
        }
        DictationSpec {
            profile_id: profile.id.clone(),
            language: profile.language.clone(),
            model_id: profile.model_id.clone(),
            model_path: model.path.clone(),
            scale_audio_ctx: model.scale_audio_ctx,
            vad_model_path: None,
        }
    };
    engine.dictation.start(DictationSpec {
        vad_model_path: vad_model_path(app, engine),
        ..spec
    });
}

/// Managed Silero VAD file when installed; otherwise kick off a silent
/// background download (2 MB) so a later dictation gets it. Dictation works
/// without it — partials and trimming just wait for the model.
fn vad_model_path(app: &AppHandle, engine: &Engine) -> Option<String> {
    let dir = app.path().app_data_dir().ok()?.join("models");
    let path = speakly_engine::models::download::dest_path(&dir, "vad-silero");
    if path.is_file() {
        return Some(path.to_string_lossy().into_owned());
    }
    if !engine.models.is_downloading("vad-silero") {
        if let Err(e) = engine.models.download("vad-silero", dir) {
            tracing::debug!("vad model download not started: {e}");
        }
    }
    None
}
