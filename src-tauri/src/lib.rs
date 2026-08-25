mod commands;
mod db;
mod export;
mod hud;
mod jobs_state;
mod keychain;
mod logs;
mod modifier_tap;
mod paste;
mod permissions;
mod popover;
mod settings;
mod shortcuts;
mod sink;
mod sound;
mod translation;
mod tray;

use std::sync::{Arc, Mutex};

use speakly_engine::Engine;
use tauri::{Emitter, Manager, WindowEvent};

use crate::settings::SettingsState;
use crate::sink::AppSink;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logs::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    shortcuts::handle_event(app, shortcut, event.state);
                })
                .build(),
        )
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
            commands::history_segments,
            commands::write_binary_file,
            commands::hud_is_key,
            commands::set_provider_key,
            commands::provider_key_status,
            commands::delete_provider_key,
            commands::test_translation,
            commands::get_settings_json,
            commands::update_settings,
            permissions::check_permissions,
            permissions::probe_microphone,
            permissions::open_privacy_pane,
            commands::engine_info,
            commands::set_update_auto_check,
            commands::get_log_path,
            commands::reveal_logs,
            commands::read_log_tail,
            commands::list_languages,
            commands::upsert_profile,
            commands::delete_profile,
            commands::show_main_window,
            commands::quit_app,
            commands::queue_file_jobs,
            commands::cancel_job,
            commands::rename_speaker,
            commands::export_transcript,
            commands::screen_recording_status,
            commands::open_screen_recording_settings,
            commands::meeting_list_apps,
            commands::meeting_start,
            commands::meeting_stop,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            {
                use tauri::menu::{Menu, PredefinedMenuItem, Submenu};
                let app_menu = Submenu::with_items(
                    app,
                    "Speakly",
                    true,
                    &[
                        &PredefinedMenuItem::hide(app, None)?,
                        &PredefinedMenuItem::hide_others(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::quit(app, None)?,
                    ],
                )?;
                let edit = Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &PredefinedMenuItem::undo(app, None)?,
                        &PredefinedMenuItem::redo(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::cut(app, None)?,
                        &PredefinedMenuItem::copy(app, None)?,
                        &PredefinedMenuItem::paste(app, None)?,
                        &PredefinedMenuItem::select_all(app, None)?,
                    ],
                )?;
                app.set_menu(Menu::with_items(app, &[&app_menu, &edit])?)?;
            }

            let loaded = settings::load_or_seed(&handle);
            app.manage(SettingsState(Mutex::new(loaded.clone())));
            settings::apply_general(&handle, &loaded.general);
            app.manage(jobs_state::JobsState::default());
            app.manage(modifier_tap::TapState::default());

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
            popover::ensure(&handle)?;
            tray::create(&handle)?;
            for err in shortcuts::register_all(&handle, Arc::clone(&engine), &loaded) {
                tracing::warn!("hotkey registration: {err}");
            }

            // Background update check on launch (silent unless one is found;
            // the UI listens for update://available).
            if loaded.updates.auto_check {
                let update_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    use tauri_plugin_updater::UpdaterExt;
                    match update_handle.updater() {
                        Ok(updater) => match updater.check().await {
                            Ok(Some(update)) => {
                                tracing::info!("update available: {}", update.version);
                                let _ = update_handle.emit(
                                    "update://available",
                                    serde_json::json!({ "version": update.version }),
                                );
                            }
                            Ok(None) => tracing::debug!("up to date"),
                            Err(e) => tracing::debug!("update check failed: {e}"),
                        },
                        Err(e) => tracing::debug!("updater unavailable: {e}"),
                    }
                });
            }

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
            // The popover dismisses itself when it loses focus.
            if window.label() == popover::POPOVER_LABEL {
                if let WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                // ggml's Metal device asserts inside C++ static destructors at
                // exit; skip them — the OS reclaims everything anyway.
                unsafe { libc::_exit(0) };
            }
        });
}
