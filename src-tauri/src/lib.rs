mod commands;
mod db;
mod hud;
mod keychain;
mod paste;
mod permissions;
mod settings;
mod shortcuts;
mod sink;
mod sound;
mod translation;
mod tray;

use std::sync::{Arc, Mutex};

use speakly_engine::Engine;
use tauri::{Manager, WindowEvent};

use crate::settings::SettingsState;
use crate::sink::AppSink;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_profiles,
            commands::get_model_status,
            commands::accessibility_status,
            commands::open_accessibility_settings,
            commands::list_models,
            commands::download_model,
            commands::cancel_download,
            commands::delete_model,
            commands::history_search,
            commands::history_delete,
            commands::history_clear,
            commands::set_provider_key,
            commands::provider_key_status,
            commands::delete_provider_key,
            commands::test_translation,
            commands::get_settings_json,
            commands::update_settings,
            permissions::check_permissions,
            permissions::probe_microphone,
            permissions::open_privacy_pane,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let loaded = settings::load_or_seed(&handle);
            app.manage(SettingsState(Mutex::new(loaded.clone())));
            settings::apply_general(&handle, &loaded.general);

            match db::Db::open_default(&handle) {
                Ok(db) => {
                    app.manage(db);
                    // Retention purge at startup, then daily.
                    let purge_handle = handle.clone();
                    std::thread::spawn(move || loop {
                        db::run_retention_purge(&purge_handle);
                        std::thread::sleep(std::time::Duration::from_secs(24 * 3600));
                    });
                }
                Err(e) => tracing::warn!("history db unavailable: {e}"),
            }

            let engine = Arc::new(Engine::new(Arc::new(AppSink {
                app: handle.clone(),
            })));
            app.manage(Arc::clone(&engine));

            hud::ensure(&handle)?;
            tray::create(&handle)?;
            shortcuts::register_all(&handle, Arc::clone(&engine), &loaded);

            // Warm the default model off the main thread; first dictation is
            // then instant instead of paying the multi-second cold load.
            if let Some(preload_id) = loaded.preload_model_id.clone() {
                if let Some(model) = loaded.models.get(&preload_id).cloned() {
                    if !model.path.is_empty() {
                        let stt = engine.stt.clone();
                        std::thread::spawn(move || match stt.preload(&preload_id, &model.path) {
                            Ok(ms) => tracing::info!("preloaded {preload_id} in {ms} ms"),
                            Err(e) => tracing::warn!("preload {preload_id}: {e}"),
                        });
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Menu-bar app behavior: closing the main window hides it.
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
