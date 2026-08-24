use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use speakly_engine::models::{download::dest_path, registry};
use speakly_engine::Engine;
use tauri::{AppHandle, Manager, State};

use crate::paste::accessibility_trusted;
use crate::settings::SettingsState;

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|d| d.join("models"))
        .map_err(|e| format!("app data dir: {e}"))
}

#[tauri::command]
pub fn get_profiles(state: State<'_, SettingsState>) -> Vec<speakly_engine_types::Profile> {
    state.0.lock().unwrap().profiles.clone()
}

/// Object-merge `patch` into `base` recursively; non-objects replace.
fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                deep_merge(base_map.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (base_slot, replacement) => *base_slot = replacement.clone(),
    }
}

#[tauri::command]
pub fn get_settings_json(state: State<'_, SettingsState>) -> Result<Value, String> {
    serde_json::to_value(&*state.0.lock().unwrap()).map_err(|e| e.to_string())
}

/// Deep-merge a JSON patch into the typed settings (the serde round-trip is
/// the validation), persist, apply side effects, broadcast to all windows.
#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
    patch: Value,
) -> Result<Value, String> {
    let updated = {
        let mut settings = state.0.lock().unwrap();
        let mut merged = serde_json::to_value(&*settings).map_err(|e| e.to_string())?;
        deep_merge(&mut merged, &patch);
        let new: crate::settings::Settings =
            serde_json::from_value(merged).map_err(|e| format!("invalid settings: {e}"))?;
        *settings = new.clone();
        crate::settings::save(&app, &settings);
        new
    };
    crate::settings::apply_general(&app, &updated.general);
    let value = serde_json::to_value(&updated).map_err(|e| e.to_string())?;
    use tauri::Emitter;
    let _ = app.emit("settings://changed", value.clone());
    Ok(value)
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
pub fn history_search(app: AppHandle, query: Option<String>, page: u32) -> Result<Value, String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or("history unavailable")?;
    let conn = db.0.lock().unwrap();
    crate::db::search(&conn, query.as_deref(), page).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or("history unavailable")?;
    let conn = db.0.lock().unwrap();
    crate::db::delete(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_clear(app: AppHandle) -> Result<(), String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or("history unavailable")?;
    let conn = db.0.lock().unwrap();
    crate::db::clear(&conn).map_err(|e| e.to_string())
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

#[tauri::command]
pub fn list_models(
    app: AppHandle,
    state: State<'_, SettingsState>,
    engine: State<'_, Arc<Engine>>,
) -> Result<Value, String> {
    let dir = models_dir(&app)?;
    let settings = state.0.lock().unwrap();
    let models: Vec<Value> = registry::REGISTRY
        .iter()
        .map(|info| {
            let managed = dest_path(&dir, info.id);
            let settings_path = settings
                .models
                .get(info.id)
                .map(|m| m.path.clone())
                .unwrap_or_default();
            let path = if managed.is_file() {
                managed.to_string_lossy().into_owned()
            } else if !settings_path.is_empty() && std::path::Path::new(&settings_path).is_file() {
                settings_path
            } else {
                String::new()
            };
            let used_by: Vec<String> = settings
                .profiles
                .iter()
                .filter(|p| p.model_id == info.id)
                .map(|p| p.name.clone())
                .collect();
            json!({
                "id": info.id,
                "name": info.name,
                "sizeBytes": info.size_bytes,
                "languages": info.languages,
                "license": info.license,
                "installed": !path.is_empty(),
                "path": path,
                "downloading": engine.models.is_downloading(info.id),
                "usedBy": used_by,
            })
        })
        .collect();
    Ok(json!({ "models": models }))
}

#[tauri::command]
pub fn download_model(
    app: AppHandle,
    engine: State<'_, Arc<Engine>>,
    id: String,
) -> Result<(), String> {
    let dir = models_dir(&app)?;
    engine.models.download(&id, dir)
}

#[tauri::command]
pub fn cancel_download(engine: State<'_, Arc<Engine>>, id: String) {
    engine.models.cancel(&id);
}

#[tauri::command]
pub fn delete_model(
    app: AppHandle,
    state: State<'_, SettingsState>,
    engine: State<'_, Arc<Engine>>,
    id: String,
) -> Result<Value, String> {
    if engine.models.is_downloading(&id) {
        return Err("cancel the running download first".into());
    }
    let dir = models_dir(&app)?;
    // Only ever delete the managed file; never a user-provided path.
    let managed = dest_path(&dir, &id);
    if managed.is_file() {
        std::fs::remove_file(&managed).map_err(|e| format!("delete: {e}"))?;
    }
    let mut warning: Option<String> = None;
    {
        let mut settings = state.0.lock().unwrap();
        let managed_str = managed.to_string_lossy();
        if let Some(entry) = settings.models.get_mut(&id) {
            if entry.path == managed_str {
                entry.path = String::new();
            }
        }
        let users: Vec<String> = settings
            .profiles
            .iter()
            .filter(|p| p.model_id == id)
            .map(|p| p.name.clone())
            .collect();
        if !users.is_empty() {
            warning = Some(format!(
                "Profiles using this model will not start until it is reinstalled: {}",
                users.join(", ")
            ));
        }
        crate::settings::save(&app, &settings);
    }
    Ok(json!({ "warning": warning }))
}

#[tauri::command]
pub fn engine_info(app: AppHandle, state: State<'_, SettingsState>) -> Value {
    let settings = state.0.lock().unwrap();
    let models: Vec<Value> = settings
        .models
        .iter()
        .map(|(id, m)| {
            json!({
                "id": id,
                "present": !m.path.is_empty() && std::path::Path::new(&m.path).is_file(),
            })
        })
        .collect();
    json!({
        "version": app.package_info().version.to_string(),
        "backend": "whisper.cpp · Metal",
        "autoCheckUpdates": settings.updates.auto_check,
        "models": models,
        "logDir": crate::logs::log_dir().to_string_lossy(),
    })
}

#[tauri::command]
pub fn set_update_auto_check(
    app: AppHandle,
    state: State<'_, SettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = state.0.lock().unwrap();
    settings.updates.auto_check = enabled;
    crate::settings::save(&app, &settings);
    Ok(())
}

#[tauri::command]
pub fn get_log_path() -> String {
    crate::logs::current_log_file()
        .unwrap_or_else(crate::logs::log_dir)
        .to_string_lossy()
        .into_owned()
}

#[tauri::command]
pub fn reveal_logs() -> Result<(), String> {
    let target = crate::logs::current_log_file().unwrap_or_else(crate::logs::log_dir);
    tauri_plugin_opener::reveal_item_in_dir(target).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_log_tail(lines: u32) -> Result<String, String> {
    crate::logs::tail(lines as usize)
}
