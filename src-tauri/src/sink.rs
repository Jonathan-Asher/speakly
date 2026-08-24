//! The engine→app bridge: receives EngineEvents on engine threads, forwards
//! them to the UI as Tauri events, drives tray/HUD state, and performs the
//! app-side half of dictation (paste) when a transcript is ready.

use serde_json::json;
use speakly_engine::{EngineEvent, EventSink, Phase};
use tauri::{AppHandle, Emitter, Manager};

use crate::paste::{paste_text, PasteOutcome};
use crate::settings::SettingsState;
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
            EngineEvent::TranscriptReady {
                profile_id,
                text,
                utterance_ms,
                decode_ms,
                latency_ms,
            } => {
                let (auto_paste, restore) = {
                    let state = self.app.state::<SettingsState>();
                    let settings = state.0.lock().unwrap();
                    settings
                        .profile(&profile_id)
                        .map(|p| (p.auto_paste, p.restore_clipboard))
                        .unwrap_or((true, true))
                };

                self.emit_state("pasting", &profile_id);

                let app = self.app.clone();
                let pid = profile_id.clone();
                let _ = self.app.run_on_main_thread(move || {
                    let outcome = if auto_paste {
                        paste_text(&app, &text, restore)
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
                    // Persist the finished dictation (history); no-op when disabled.
                    crate::db::persist_dictation(&app, &pid, &text, utterance_ms, None);
                    tray::set_state(&app, "idle");
                    let _ = app.emit(
                        "dictation://final",
                        json!({
                            "profileId": pid,
                            "text": text,
                            "phase": phase,
                            "note": note,
                            "utteranceMs": utterance_ms,
                            "decodeMs": decode_ms,
                            "latencyMs": latency_ms,
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
