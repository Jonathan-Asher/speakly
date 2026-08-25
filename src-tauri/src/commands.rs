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
pub fn history_search(
    app: AppHandle,
    query: Option<String>,
    kind: Option<String>,
    page: u32,
) -> Result<Value, String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or("history unavailable")?;
    let conn = db.0.lock().unwrap();
    crate::db::search(&conn, query.as_deref(), kind.as_deref(), page).map_err(|e| e.to_string())
}

/// Timestamped (and speaker-labeled, when present) segments of a stored
/// file/meeting transcript.
#[tauri::command]
pub fn history_segments(app: AppHandle, transcript_id: i64) -> Result<Vec<Value>, String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or("history unavailable")?;
    let conn = db.0.lock().unwrap();
    crate::db::segments_for(&conn, transcript_id).map_err(|e| e.to_string())
}

/// Write frontend-rendered binary export data (docx/pdf) to a path the user
/// picked in the save dialog. Provenance of `path` cannot be proven here —
/// the trade-off (vs. an fs-scope dance) is documented in the plan; the
/// command writes only regular files and never creates directories.
#[tauri::command]
pub fn write_binary_file(path: String, base64_data: String) -> Result<(), String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data.as_bytes())
        .map_err(|e| format!("decode: {e}"))?;
    std::fs::write(&path, bytes).map_err(|e| format!("write: {e}"))?;
    Ok(())
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

/// QA probe: the HUD must never be the key window (it would steal the paste
/// target's focus). Checked on the main thread.
#[tauri::command]
pub fn hud_is_key(app: AppHandle) -> Result<bool, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(crate::hud::is_key_window(&handle));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
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
#[allow(clippy::too_many_arguments)]
pub fn queue_file_jobs(
    app: AppHandle,
    state: State<'_, SettingsState>,
    engine: State<'_, Arc<Engine>>,
    paths: Vec<String>,
    language: String,
    model_id: String,
    diarize: Option<bool>,
    num_speakers: Option<u32>,
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
    let diarize_opts = if diarize.unwrap_or(false) {
        Some(diarize_opts(&app, &state, num_speakers)?)
    } else {
        None
    };

    let mut ids = Vec::with_capacity(paths.len());
    for path in paths {
        let id = engine.jobs.enqueue(speakly_engine::jobs::QueueOptions {
            path: path.clone(),
            language: language.clone(),
            model_id: model_id.clone(),
            model_path: model_path.clone(),
            scale_audio_ctx: scale,
            diarize: diarize_opts.clone(),
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
        .filter(|info| !info.hidden)
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

// ---- Meetings ----

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> u8;
}

const SIDECAR_NAME: &str = "speakly-syscap-aarch64-apple-darwin";

/// Dev builds use the repo-staged binary; bundled builds the Resources copy.
fn sidecar_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(res) = app.path().resource_dir() {
        let bundled = res.join(SIDECAR_NAME);
        if bundled.is_file() {
            return Ok(bundled);
        }
    }
    let staged = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(SIDECAR_NAME);
    if staged.is_file() {
        return Ok(staged);
    }
    Err("meeting sidecar not found — run scripts/build-sidecar.sh".into())
}

#[tauri::command]
pub fn screen_recording_status() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() != 0 }
}

#[tauri::command]
pub fn open_screen_recording_settings() {
    let _ = tauri_plugin_opener::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        None::<String>,
    );
}

#[tauri::command]
pub fn meeting_list_apps(app: AppHandle) -> Result<Value, String> {
    let path = sidecar_path(&app)?;
    let out = std::process::Command::new(&path)
        .arg("--list-apps")
        .output()
        .map_err(|e| format!("run sidecar: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "sidecar exited with {:?} — is Screen Recording granted?",
            out.status.code()
        ));
    }
    serde_json::from_slice::<Value>(&out.stdout).map_err(|e| format!("parse app list: {e}"))
}

/// Resolve the two diarization model files (managed dir first, then any
/// settings-recorded path). Errors with a download hint when missing.
fn diarize_opts(
    app: &AppHandle,
    state: &State<'_, SettingsState>,
    num_speakers: Option<u32>,
) -> Result<speakly_engine::diarize::DiarizeOpts, String> {
    let dir = models_dir(app)?;
    let resolve = |id: &str| -> Result<String, String> {
        let managed = dest_path(&dir, id);
        if managed.is_file() {
            return Ok(managed.to_string_lossy().into_owned());
        }
        let settings = state.0.lock().unwrap();
        if let Some(entry) = settings.models.get(id) {
            if !entry.path.is_empty() && std::path::Path::new(&entry.path).is_file() {
                return Ok(entry.path.clone());
            }
        }
        Err(format!(
            "speaker identification needs the '{id}' model — download it in Models first"
        ))
    };
    Ok(speakly_engine::diarize::DiarizeOpts {
        seg_model_path: resolve("diar-seg")?,
        emb_model_path: resolve("diar-emb")?,
        num_speakers: num_speakers.map(|n| n as usize),
    })
}

#[tauri::command]
pub fn rename_speaker(
    app: AppHandle,
    transcript_id: i64,
    from: String,
    to: String,
) -> Result<usize, String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or("history unavailable")?;
    let conn = db.0.lock().unwrap();
    crate::db::rename_speaker(&conn, transcript_id, &from, &to).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
pub struct MeetingStartArgs {
    pub apps: Vec<String>,
    pub system: bool,
    pub mic: bool,
    pub model_id: String,
    pub language: String,
    #[serde(default)]
    pub diarize: bool,
    #[serde(default)]
    pub num_speakers: Option<u32>,
}

#[tauri::command]
pub fn meeting_start(
    app: AppHandle,
    state: State<'_, SettingsState>,
    engine: State<'_, Arc<Engine>>,
    args: MeetingStartArgs,
) -> Result<u64, String> {
    let sidecar = sidecar_path(&app)?;
    let (model_path, scale_audio_ctx) = {
        let settings = state.0.lock().unwrap();
        let model = settings.models.get(&args.model_id).ok_or("unknown model")?;
        if model.path.is_empty() || !std::path::Path::new(&model.path).is_file() {
            return Err(format!("model {} is not installed", args.model_id));
        }
        (model.path.clone(), model.scale_audio_ctx)
    };
    let diarize = if args.diarize {
        Some(diarize_opts(&app, &state, args.num_speakers)?)
    } else {
        None
    };
    engine.meetings.start(speakly_engine::MeetingOpts {
        sidecar_path: sidecar.to_string_lossy().into_owned(),
        bundle_ids: args.apps,
        system: args.system,
        mic: args.mic,
        model_id: args.model_id,
        model_path,
        language: args.language,
        scale_audio_ctx,
        diarize,
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

/// Create or update a dictation profile, persist, and re-register hotkeys.
/// Validation failures return specific messages for inline display.
#[tauri::command]
pub fn upsert_profile(
    app: AppHandle,
    state: State<'_, SettingsState>,
    engine: State<'_, Arc<Engine>>,
    profile: speakly_engine_types::Profile,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::Shortcut;

    if profile.id.trim().is_empty() {
        return Err("Profile id missing".into());
    }
    if profile.name.trim().is_empty() {
        return Err("Give the profile a name".into());
    }
    if profile.language.trim().is_empty() {
        return Err("Pick a language".into());
    }
    // Bare-modifier specs ("RightOption" etc.) are valid but live outside the
    // plugin's accelerator grammar; combos must parse.
    let is_bare = crate::modifier_tap::parse_bare(&profile.hotkey).is_some();
    let parsed: Option<Shortcut> = if is_bare {
        None
    } else {
        Some(
            profile
                .hotkey
                .parse()
                .map_err(|_| format!("'{}' is not a usable hotkey", profile.hotkey))?,
        )
    };
    if let Some(t) = profile.translate.as_ref().filter(|t| t.enabled) {
        if matches!(
            t.provider,
            speakly_engine_types::TranslationProvider::Custom
        ) && t.endpoint.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err("The custom provider needs an endpoint URL".into());
        }
        if t.target_language.trim().is_empty() {
            return Err("Pick a target language for translation".into());
        }
    }

    let snapshot = {
        let mut settings = state.0.lock().unwrap();
        if !settings.models.contains_key(&profile.model_id) {
            return Err(format!("Unknown model '{}'", profile.model_id));
        }
        let clash = settings.profiles.iter().any(|p| {
            p.id != profile.id
                && match &parsed {
                    Some(parsed) => p
                        .hotkey
                        .parse::<Shortcut>()
                        .map(|s| s == *parsed)
                        .unwrap_or(false),
                    None => p.hotkey == profile.hotkey,
                }
        });
        if clash {
            return Err(format!(
                "'{}' is already used by another profile",
                profile.hotkey
            ));
        }
        match settings.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => *existing = profile.clone(),
            None => settings.profiles.push(profile.clone()),
        }
        crate::settings::save(&app, &settings);
        settings.clone()
    };

    let errors = crate::shortcuts::reregister(&app, Arc::clone(&engine), &snapshot);
    let _ = tauri::Emitter::emit(
        &app,
        "settings://changed",
        serde_json::to_value(&snapshot).unwrap_or(Value::Null),
    );
    if let Some(e) = errors.iter().find(|e| e.contains(&profile.hotkey)) {
        return Err(format!("Saved, but the hotkey did not take: {e}"));
    }
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

#[tauri::command]
pub fn delete_profile(
    app: AppHandle,
    state: State<'_, SettingsState>,
    engine: State<'_, Arc<Engine>>,
    id: String,
) -> Result<(), String> {
    let snapshot = {
        let mut settings = state.0.lock().unwrap();
        if settings.profiles.len() <= 1 {
            return Err("At least one profile must remain".into());
        }
        let before = settings.profiles.len();
        settings.profiles.retain(|p| p.id != id);
        if settings.profiles.len() == before {
            return Err("Profile not found".into());
        }
        crate::settings::save(&app, &settings);
        settings.clone()
    };
    crate::shortcuts::reregister(&app, Arc::clone(&engine), &snapshot);
    let _ = tauri::Emitter::emit(
        &app,
        "settings://changed",
        serde_json::to_value(&snapshot).unwrap_or(Value::Null),
    );
    Ok(())
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
    if let Some(pop) = app.get_webview_window(crate::popover::POPOVER_LABEL) {
        let _ = pop.hide();
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn meeting_stop(engine: State<'_, Arc<Engine>>, session_id: u64) -> Result<(), String> {
    engine.meetings.stop(session_id)
}
