//! Single settings store: one JSON file in the app config dir, typed structs,
//! seeded defaults on first run. Never contains secrets (keys go to Keychain
//! when translation lands).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use speakly_engine_types::{DictationMode, Profile, TranslateConfig, TranslationProvider};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub path: String,
    /// Enable the encoder-context speedup for this model (validated per model).
    pub scale_audio_ctx: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySettings {
    pub enabled: bool,
    /// Dictation snippets are the most privacy-sensitive kind; separately
    /// toggleable from history as a whole.
    pub save_dictation: bool,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            save_dictation: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatesSettings {
    pub auto_check: bool,
}

impl Default for UpdatesSettings {
    fn default() -> Self {
        Self { auto_check: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub profiles: Vec<Profile>,
    pub models: HashMap<String, ModelEntry>,
    /// Model loaded into the warm pool at app start.
    pub preload_model_id: Option<String>,
    #[serde(default)]
    pub history: HistorySettings,
    #[serde(default)]
    pub updates: UpdatesSettings,
}

pub struct SettingsState(pub Mutex<Settings>);

impl Settings {
    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }
}

fn settings_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .expect("app config dir")
        .join("settings.json")
}

pub fn load_or_seed(app: &AppHandle) -> Settings {
    let path = settings_path(app);
    if let Ok(bytes) = std::fs::read(&path) {
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(mut s) => {
                if migrate(&mut s) {
                    save(app, &s);
                }
                return s;
            }
            Err(e) => tracing::warn!("settings unreadable ({e}), reseeding"),
        }
    }
    let seeded = seed();
    save(app, &seeded);
    seeded
}

/// Settings written by an older build get new seeded content appended.
/// Returns true when anything changed.
fn migrate(s: &mut Settings) -> bool {
    let has_translate_profile = s
        .profiles
        .iter()
        .any(|p| p.id == "he-en" || p.translate.as_ref().is_some_and(|t| t.enabled));
    if !has_translate_profile {
        s.profiles.push(he_en_profile());
        return true;
    }
    false
}

pub fn save(app: &AppHandle, settings: &Settings) {
    let path = settings_path(app);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_vec_pretty(settings) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!("settings save failed: {e}");
            }
        }
        Err(e) => tracing::warn!("settings serialize failed: {e}"),
    }
}

/// First-run defaults: Hebrew and English hold-to-talk profiles. Model paths
/// point at known local files when present; the in-app model manager (P1
/// follow-up) replaces this with managed downloads into the app data dir.
fn seed() -> Settings {
    // Managed (model-manager) locations first, then dev-machine fallbacks.
    let he_candidates = [
        format!(
            "{}/Library/Application Support/com.speakly.app/models/ggml-he-turbo.bin",
            home()
        ),
        format!(
            "{}/Documents/Work/speakly/models-dev/ggml-ivrit-large-v3-turbo.bin",
            home()
        ),
    ];
    let en_candidates = [format!(
        "{}/Library/Application Support/com.speakly.app/models/ggml-turbo.bin",
        home()
    )];

    let mut models = HashMap::new();
    models.insert(
        "he-turbo".to_string(),
        ModelEntry {
            path: first_existing(&he_candidates),
            // Off until validated on real-microphone Hebrew (plan: per-model flag).
            scale_audio_ctx: false,
        },
    );
    models.insert(
        "turbo".to_string(),
        ModelEntry {
            path: first_existing(&en_candidates),
            scale_audio_ctx: true,
        },
    );

    Settings {
        profiles: vec![
            Profile {
                id: "he".into(),
                name: "Hebrew".into(),
                hotkey: "Alt+Space".into(),
                mode: DictationMode::Hold,
                language: "he".into(),
                model_id: "he-turbo".into(),
                translate: None,
                auto_paste: true,
                restore_clipboard: true,
            },
            Profile {
                id: "en".into(),
                name: "English".into(),
                hotkey: "Shift+Alt+Space".into(),
                mode: DictationMode::Hold,
                language: "en".into(),
                model_id: "turbo".into(),
                translate: None,
                auto_paste: true,
                restore_clipboard: true,
            },
            he_en_profile(),
        ],
        models,
        preload_model_id: Some("he-turbo".into()),
        history: HistorySettings::default(),
        updates: UpdatesSettings::default(),
    }
}

/// The signature Speakly flow: dictate Hebrew, English lands at the cursor.
fn he_en_profile() -> Profile {
    Profile {
        id: "he-en".into(),
        name: "Hebrew → English".into(),
        hotkey: "Ctrl+Alt+Space".into(),
        mode: DictationMode::Hold,
        language: "he".into(),
        model_id: "he-turbo".into(),
        translate: Some(TranslateConfig {
            enabled: true,
            provider: TranslationProvider::Groq,
            target_language: "English".into(),
            system_prompt: None,
            model: None,
            endpoint: None,
        }),
        auto_paste: true,
        restore_clipboard: true,
    }
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn first_existing(candidates: &[String]) -> String {
    candidates
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .cloned()
        .unwrap_or_default()
}
