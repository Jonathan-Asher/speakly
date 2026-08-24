use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

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

#[tauri::command]
pub fn open_accessibility_settings(app: AppHandle) {
    let _ = tauri_plugin_opener::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        None::<String>,
    );
    let _ = app;
}
