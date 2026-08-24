use serde_json::{json, Value};
use tauri::{AppHandle, State};

use crate::paste::accessibility_trusted;
use crate::settings::SettingsState;

#[tauri::command]
pub fn get_profiles(state: State<'_, SettingsState>) -> Vec<speakly_engine_types::Profile> {
    state.0.lock().unwrap().profiles.clone()
}

#[tauri::command]
pub fn get_model_status(state: State<'_, SettingsState>) -> Value {
    let settings = state.0.lock().unwrap();
    let models: Vec<Value> = settings
        .models
        .iter()
        .map(|(id, m)| {
            json!({
                "id": id,
                "path": m.path,
                "present": !m.path.is_empty() && std::path::Path::new(&m.path).is_file(),
            })
        })
        .collect();
    json!({ "models": models })
}

#[tauri::command]
pub fn accessibility_status() -> bool {
    accessibility_trusted()
}

fn valid_provider(provider: &str) -> Result<(), String> {
    crate::translation::parse_provider(provider)
        .map(|_| ())
        .ok_or_else(|| format!("unknown provider: {provider}"))
}

#[tauri::command]
pub fn set_provider_key(provider: String, key: String) -> Result<(), String> {
    valid_provider(&provider)?;
    if key.trim().is_empty() {
        return Err("key is empty".into());
    }
    crate::keychain::set_key(&provider, &key)
}

#[tauri::command]
pub fn provider_key_status(provider: String) -> Result<Value, String> {
    valid_provider(&provider)?;
    let (present, last4) = crate::keychain::key_status(&provider)?;
    Ok(json!({ "present": present, "last4": last4 }))
}

#[tauri::command]
pub fn delete_provider_key(provider: String) -> Result<(), String> {
    valid_provider(&provider)?;
    crate::keychain::delete_key(&provider)
}

/// Round-trip check: translates a short Hebrew phrase with the saved key.
#[tauri::command]
pub fn test_translation(provider: String, target_language: String) -> Result<String, String> {
    let parsed = crate::translation::parse_provider(&provider)
        .ok_or_else(|| format!("unknown provider: {provider}"))?;
    let key = crate::keychain::get_key(&provider)?.unwrap_or_default();
    if key.is_empty() && !matches!(parsed, speakly_engine_types::TranslationProvider::Custom) {
        return Err("No API key saved for this provider".into());
    }
    let cfg = speakly_engine_types::TranslateConfig {
        enabled: true,
        provider: parsed,
        target_language,
        system_prompt: None,
        model: None,
        endpoint: None,
    };
    crate::translation::translate(&cfg, &key, "שלום עולם")
}

#[tauri::command]
pub fn open_accessibility_settings(app: AppHandle) {
    let _ = tauri_plugin_opener::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        None::<String>,
    );
    let _ = app;
}
