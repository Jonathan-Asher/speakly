//! Speakly engine: audio capture, VAD, speech-to-text inference, scheduling,
//! model management, diarization, and meeting capture. UI-free — the Tauri app
//! crate drives it and receives events through [`EventSink`].

pub mod audio;
pub mod dictation;
pub mod meeting;
pub mod models;
pub mod stt;

use std::sync::Arc;

pub use dictation::{DictationEngine, DictationSpec};
pub use meeting::{MeetingOpts, MeetingService};
pub use models::ModelService;
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
    ModelProgress {
        id: String,
        bytes: u64,
        total: Option<u64>,
        bps: u64,
    },
    ModelReady {
        id: String,
        path: String,
    },
    ModelError {
        id: String,
        message: String,
    },
    MeetingStatus {
        session_id: u64,
        state: String,
        message: Option<String>,
    },
    MeetingSegment {
        session_id: u64,
        t0_ms: u64,
        t1_ms: u64,
        text: String,
        source: String,
    },
    /// Emitted once per session after the final window flush; the app layer
    /// persists the concatenated transcript to history.
    MeetingFinished {
        session_id: u64,
        text: String,
        duration_ms: u64,
    },
}

/// Facade owning the engine services. One per app.
pub struct Engine {
    pub stt: SttService,
    pub dictation: DictationEngine,
    pub models: ModelService,
    pub meetings: MeetingService,
}

impl Engine {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        let stt = SttService::spawn();
        let models = ModelService::new(Arc::clone(&sink));
        let meetings = MeetingService::new(stt.clone(), Arc::clone(&sink));
        let dictation = DictationEngine::new(stt.clone(), sink);
        Self {
            stt,
            dictation,
            models,
            meetings,
        }
    }
}
