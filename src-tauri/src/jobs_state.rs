//! App-side bookkeeping for file jobs: metadata registered at queue time,
//! segments accumulated as the engine streams them, and persistence into
//! history when a job completes. Keeps `sink.rs` arms one-liners.

use std::collections::HashMap;
use std::sync::Mutex;

use speakly_engine_types::Segment;
use tauri::{AppHandle, Manager};

pub struct JobMeta {
    pub path: String,
    pub model_id: String,
    pub language: String,
}

#[derive(Default)]
pub struct JobEntry {
    meta: Option<JobMeta>,
    segments: Vec<Segment>,
}

#[derive(Default)]
pub struct JobsState(pub Mutex<HashMap<String, JobEntry>>);

pub fn register(app: &AppHandle, id: &str, meta: JobMeta) {
    if let Some(state) = app.try_state::<JobsState>() {
        state.0.lock().unwrap().insert(
            id.to_string(),
            JobEntry {
                meta: Some(meta),
                segments: Vec::new(),
            },
        );
    }
}

pub fn on_segment(app: &AppHandle, id: &str, segment: &Segment) {
    if let Some(state) = app.try_state::<JobsState>() {
        if let Some(entry) = state.0.lock().unwrap().get_mut(id) {
            entry.segments.push(segment.clone());
        }
    }
}

/// Called on done/error/cancel. Persists to history only on done
/// (`done_duration_ms` present) and only when history is enabled.
pub fn on_terminal(app: &AppHandle, id: &str, done_duration_ms: Option<u64>) {
    let Some(state) = app.try_state::<JobsState>() else {
        return;
    };
    let Some(entry) = state.0.lock().unwrap().remove(id) else {
        return;
    };
    let Some(duration_ms) = done_duration_ms else {
        return;
    };
    let Some(meta) = entry.meta else { return };
    let segments = entry.segments;
    if segments.is_empty() {
        return;
    }

    let history_enabled = {
        let settings = app.state::<crate::settings::SettingsState>();
        let s = settings.0.lock().unwrap();
        s.history.enabled
    };
    if !history_enabled {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        let Some(db) = app.try_state::<crate::db::Db>() else {
            return;
        };
        let conn = db.0.lock().unwrap();
        let text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        match crate::db::insert_transcript(
            &conn,
            "file",
            None,
            Some(&meta.model_id),
            Some(&meta.language),
            duration_ms as i64,
            &text,
            None,
        ) {
            Ok(transcript_id) => {
                if let Err(e) = crate::db::set_source_path(&conn, transcript_id, &meta.path) {
                    tracing::warn!("source path update failed: {e}");
                }
                if let Err(e) = crate::db::insert_segments(&conn, transcript_id, &segments) {
                    tracing::warn!("segment insert failed: {e}");
                }
            }
            Err(e) => tracing::warn!("file transcript insert failed: {e}"),
        }
    });
}
