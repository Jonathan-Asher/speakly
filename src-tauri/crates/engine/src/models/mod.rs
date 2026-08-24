//! Model management: static registry + resumable downloads running on
//! per-download threads, reporting through the EventSink.

pub mod download;
pub mod registry;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::{EngineEvent, EventSink};

pub struct ModelService {
    sink: Arc<dyn EventSink>,
    active: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl ModelService {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        Self {
            sink,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn is_downloading(&self, id: &str) -> bool {
        self.active.lock().unwrap().contains_key(id)
    }

    /// Start (or resume) a download into `dir` on a background thread.
    pub fn download(&self, id: &str, dir: PathBuf) -> Result<(), String> {
        let info = registry::get(id).ok_or_else(|| format!("unknown model: {id}"))?;
        {
            let mut active = self.active.lock().unwrap();
            if active.contains_key(id) {
                return Err("download already running".into());
            }
            active.insert(id.to_string(), Arc::new(AtomicBool::new(false)));
        }
        let cancel = self.active.lock().unwrap().get(id).unwrap().clone();
        let sink = Arc::clone(&self.sink);
        let active = Arc::clone(&self.active);
        let id_owned = id.to_string();

        std::thread::Builder::new()
            .name(format!("speakly-dl-{id_owned}"))
            .spawn(move || {
                let result = download::download(info, &dir, &cancel, |bytes, total, bps| {
                    sink.emit(EngineEvent::ModelProgress {
                        id: id_owned.clone(),
                        bytes,
                        total,
                        bps,
                    });
                });
                active.lock().unwrap().remove(&id_owned);
                match result {
                    Ok(path) => sink.emit(EngineEvent::ModelReady {
                        id: id_owned,
                        path: path.to_string_lossy().into_owned(),
                    }),
                    Err(message) => sink.emit(EngineEvent::ModelError {
                        id: id_owned,
                        message,
                    }),
                }
            })
            .expect("spawn download thread");
        Ok(())
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.active.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}
