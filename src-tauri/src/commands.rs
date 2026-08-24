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

/// The one list of transcription languages, served from Rust.
#[tauri::command]
pub fn list_languages() -> Value {
    let langs: &[(&str, &str)] = &[
        ("auto", "Auto-detect"),
        ("he", "Hebrew"),
        ("en", "English"),
        ("es", "Spanish"),
        ("fr", "French"),
        ("de", "German"),
        ("it", "Italian"),
        ("pt", "Portuguese"),
        ("zh", "Chinese"),
        ("ja", "Japanese"),
        ("ko", "Korean"),
        ("ru", "Russian"),
        ("ar", "Arabic"),
        ("hi", "Hindi"),
    ];
    json!(langs
        .iter()
        .map(|(code, label)| json!({ "code": code, "label": label }))
        .collect::<Vec<_>>())
}

#[tauri::command]
pub fn queue_file_jobs(
    app: AppHandle,
    state: State<'_, SettingsState>,
    engine: State<'_, Arc<Engine>>,
    paths: Vec<String>,
    language: String,
    model_id: String,
) -> Result<Vec<String>, String> {
    let (model_path, scale) = {
        let settings = state.0.lock().unwrap();
        let entry = settings
            .models
            .get(&model_id)
            .ok_or_else(|| format!("unknown model: {model_id}"))?;
        (entry.path.clone(), entry.scale_audio_ctx)
    };
    if model_path.is_empty() || !std::path::Path::new(&model_path).is_file() {
        return Err(format!(
            "model '{model_id}' is not installed — download it in Models first"
        ));
    }

    let mut ids = Vec::with_capacity(paths.len());
    for path in paths {
        let id = engine.jobs.enqueue(speakly_engine::jobs::QueueOptions {
            path: path.clone(),
            language: language.clone(),
            model_id: model_id.clone(),
            model_path: model_path.clone(),
            scale_audio_ctx: scale,
        });
        crate::jobs_state::register(
            &app,
            &id,
            crate::jobs_state::JobMeta {
                path,
                model_id: model_id.clone(),
                language: language.clone(),
            },
        );
        ids.push(id);
    }
    Ok(ids)
}

#[tauri::command]
pub fn cancel_job(engine: State<'_, Arc<Engine>>, id: String) {
    engine.jobs.cancel(&id);
}

/// Render the given segments and save via a native dialog. Returns the saved
/// path, or None when the user cancels.
#[tauri::command]
pub fn export_transcript(
    app: AppHandle,
    format: String,
    suggested_name: String,
    segments: Vec<crate::export::ExportSegment>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let content = crate::export::render(&format, &segments)?;
    let ext = crate::export::extension(&format);
    let picked = app
        .dialog()
        .file()
        .set_file_name(format!("{suggested_name}.{ext}"))
        .add_filter(format.to_uppercase(), &[ext])
        .blocking_save_file();
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|e| format!("resolve path: {e}"))?;
    std::fs::write(&path, content).map_err(|e| format!("write: {e}"))?;
    Ok(Some(path.to_string_lossy().into_owned()))
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
