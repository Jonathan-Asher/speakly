//! Speakly engine: audio capture, VAD, speech-to-text inference, scheduling,
//! model management, diarization, and meeting capture. UI-free — the Tauri app
//! crate drives it and receives events through [`EventSink`].

pub mod audio;
pub mod dictation;
pub mod stt;

use std::sync::Arc;

pub use dictation::{DictationEngine, DictationSpec};
pub use stt::SttService;

/// The engine's only way to talk to the outside world. The app crate
/// implements this over `AppHandle::emit`; tests and CLI spikes implement it
/// over a channel or stdout.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: EngineEvent);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Listening,
    Transcribing,
    Error,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Listening => "listening",
            Phase::Transcribing => "transcribing",
            Phase::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    DictationState {
        phase: Phase,
        profile_id: String,
    },
    /// A finished utterance. The app layer decides what to do with the text
    /// (translate, paste, persist) and emits its own further phases.
    TranscriptReady {
        profile_id: String,
        text: String,
        utterance_ms: u64,
        decode_ms: u64,
        latency_ms: u64,
    },
    Warning {
        code: String,
        message: String,
    },
}

/// Facade owning the engine services. One per app.
pub struct Engine {
    pub stt: SttService,
    pub dictation: DictationEngine,
}

impl Engine {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        let stt = SttService::spawn();
        let dictation = DictationEngine::new(stt.clone(), sink);
        Self { stt, dictation }
    }
}
