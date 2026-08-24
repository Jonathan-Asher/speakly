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

pub fn register_all(app: &AppHandle, engine: Arc<Engine>, settings: &Settings) {
    for profile in &settings.profiles {
        let shortcut: Shortcut = match profile.hotkey.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "profile {}: bad hotkey '{}': {e:?}",
                    profile.id,
                    profile.hotkey
                );
                continue;
            }
        };
        let engine = Arc::clone(&engine);
        let profile_id = profile.id.clone();
        let mode = profile.mode;

        let result = app
            .global_shortcut()
            .on_shortcut(shortcut, move |app, _sc, event| match (mode, event.state) {
                (DictationMode::Hold, ShortcutState::Pressed) => {
                    start(app, &engine, &profile_id);
                }
                (DictationMode::Hold, ShortcutState::Released) => engine.dictation.stop(),
                (DictationMode::Toggle, ShortcutState::Pressed) => {
                    if engine.dictation.is_active() {
                        engine.dictation.stop();
                    } else {
                        start(app, &engine, &profile_id);
                    }
                }
                (DictationMode::Toggle, ShortcutState::Released) => {}
            });
        match result {
            Ok(()) => tracing::info!("profile {} on {}", profile.id, profile.hotkey),
            Err(e) => tracing::warn!("register {} failed: {e}", profile.hotkey),
        }
    }
}

fn start(app: &AppHandle, engine: &Engine, profile_id: &str) {
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
    engine.dictation.start(DictationSpec {
        profile_id: profile.id.clone(),
        language: profile.language.clone(),
        model_id: profile.model_id.clone(),
        model_path: model.path.clone(),
        scale_audio_ctx: model.scale_audio_ctx,
    });
}
