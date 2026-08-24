//! The engine→app bridge: receives EngineEvents on engine threads, forwards
//! them to the UI as Tauri events, drives tray/HUD state, and performs the
//! app-side half of dictation (paste) when a transcript is ready.

use serde_json::json;
use speakly_engine::{EngineEvent, EventSink, Phase};
use tauri::{AppHandle, Emitter, Manager};

use crate::paste::{paste_text, PasteOutcome};
use crate::settings::{ModelEntry, SettingsState};
use crate::{hud, tray};

pub struct AppSink {
    pub app: AppHandle,
}

impl AppSink {
    fn emit_state(&self, phase: &str, profile_id: &str) {
        tray::set_state(&self.app, phase);
        match phase {
            "listening" => hud::show(&self.app),
            "idle" => hud::hide(&self.app),
            _ => {}
        }
        let _ = self.app.emit(
            "dictation://state",
            json!({ "phase": phase, "profileId": profile_id }),
        );
    }
}

impl EventSink for AppSink {
    fn emit(&self, event: EngineEvent) {
        match event {
            EngineEvent::DictationState { phase, profile_id } => {
                self.emit_state(phase.as_str(), &profile_id);
                if phase == Phase::Error {
                    schedule_idle(&self.app, profile_id, 2_500);
                }
            }
            EngineEvent::Warning { code, message } => {
                tracing::warn!("engine warning [{code}]: {message}");
                let _ = self.app.emit(
                    "engine://warning",
                    json!({ "code": code, "message": message }),
                );
            }
            EngineEvent::ModelProgress {
                id,
                bytes,
                total,
                bps,
            } => {
                let _ = self.app.emit(
                    "model://progress",
                    json!({ "id": id, "bytes": bytes, "total": total, "bps": bps }),
                );
            }
            EngineEvent::ModelReady { id, path } => {
                {
                    let state = self.app.state::<SettingsState>();
                    let mut settings = state.0.lock().unwrap();
                    let scale_default = id != "he-turbo";
                    let entry = settings
                        .models
                        .entry(id.clone())
                        .or_insert_with(|| ModelEntry {
                            path: String::new(),
                            scale_audio_ctx: scale_default,
                        });
                    entry.path = path.clone();
                    crate::settings::save(&self.app, &settings);
                }
                tracing::info!("model {id} installed at {path}");
                let _ = self
                    .app
                    .emit("model://ready", json!({ "id": id, "path": path }));
            }
            EngineEvent::ModelError { id, message } => {
                tracing::warn!("model {id} download failed: {message}");
                let _ = self
                    .app
                    .emit("model://error", json!({ "id": id, "message": message }));
            }
            EngineEvent::TranscriptReady {
                profile_id,
                text,
                utterance_ms,
                decode_ms,
                latency_ms,
            } => {
                let (auto_paste, restore, translate_cfg) = {
                    let state = self.app.state::<SettingsState>();
                    let settings = state.0.lock().unwrap();
                    settings
                        .profile(&profile_id)
                        .map(|p| (p.auto_paste, p.restore_clipboard, p.translate.clone()))
                        .unwrap_or((true, true, None))
                };

                // Translation stage (He→En etc.) — runs on this engine thread,
                // never the main thread. Failure pastes the untranslated source.
                let mut final_text = text.clone();
                let mut translated = false;
                let mut translated_provider: Option<String> = None;
                let mut translate_ms: Option<u64> = None;
                if let Some(cfg) = translate_cfg.filter(|c| c.enabled) {
                    self.emit_state("translating", &profile_id);
                    let slug = crate::translation::provider_slug(cfg.provider);
                    let key = crate::keychain::get_key(slug)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let is_custom = matches!(
                        cfg.provider,
                        speakly_engine_types::TranslationProvider::Custom
                    );
                    if key.is_empty() && !is_custom {
                        let _ = self.app.emit(
                            "engine://warning",
                            json!({
                                "code": "translate",
                                "message": format!("No API key saved for {slug} — pasted the original text"),
                            }),
                        );
                    } else {
                        let t0 = std::time::Instant::now();
                        match crate::translation::translate(&cfg, &key, &final_text) {
                            Ok(t) => {
                                translate_ms = Some(t0.elapsed().as_millis() as u64);
                                final_text = t;
                                translated = true;
                                translated_provider = Some(slug.to_string());
                            }
                            Err(e) => {
                                translate_ms = Some(t0.elapsed().as_millis() as u64);
                                tracing::warn!("translation failed: {e}");
                                let _ = self.app.emit(
                                    "engine://warning",
                                    json!({
                                        "code": "translate",
                                        "message": format!("Translation failed ({e}) — pasted the original text"),
                                    }),
                                );
                            }
                        }
                    }
                }

                self.emit_state("pasting", &profile_id);

                let app = self.app.clone();
                let pid = profile_id.clone();
                let _ = self.app.run_on_main_thread(move || {
                    let outcome = if auto_paste {
                        paste_text(&app, &final_text, restore)
                    } else {
                        PasteOutcome::ClipboardOnly
                    };
                    let (phase, note) = match outcome {
                        PasteOutcome::Pasted => ("pasted", None),
                        PasteOutcome::ClipboardOnly => (
                            "copied",
                            Some("Copied — press ⌘V to paste (grant Accessibility for auto-paste)"),
                        ),
                        PasteOutcome::Failed(e) => {
                            tracing::warn!("paste failed: {e}");
                            ("copied", Some("Paste failed — text is on the clipboard"))
                        }
                    };
                    // Persist the finished dictation (history); the source text
                    // is the transcript, the translation rides along.
                    let translated_for_db = translated_provider
                        .as_ref()
                        .map(|p| (final_text.clone(), p.clone()));
                    crate::db::persist_dictation(
                        &app,
                        &pid,
                        &text,
                        utterance_ms,
                        translated_for_db,
                    );
                    tray::set_state(&app, "idle");
                    let _ = app.emit(
                        "dictation://final",
                        json!({
                            "profileId": pid,
                            "text": final_text,
                            "phase": phase,
                            "note": note,
                            "utteranceMs": utterance_ms,
                            "decodeMs": decode_ms,
                            "latencyMs": latency_ms,
                            "translateMs": translate_ms,
                            "translated": translated,
                        }),
                    );
                    let _ = app.emit(
                        "dictation://state",
                        json!({ "phase": phase, "profileId": pid }),
                    );
                    schedule_idle(&app, pid, 1_400);
                });
            }
        }
    }
}

/// After a terminal phase, return the HUD/UI to idle shortly.
fn schedule_idle(app: &AppHandle, profile_id: String, delay_ms: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        tray::set_state(&app, "idle");
        hud::hide(&app);
        let _ = app.emit(
            "dictation://state",
            json!({ "phase": "idle", "profileId": profile_id }),
        );
    });
}
