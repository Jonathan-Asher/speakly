//! Speakly engine: audio capture, VAD, speech-to-text inference, scheduling,
//! model management, diarization, and meeting capture. UI-free — the Tauri app
//! crate drives it and receives events through [`EventSink`].

pub mod audio;
pub mod dictation;
pub mod jobs;
pub mod meeting;
pub mod models;
pub mod stt;
pub mod vad;

use std::sync::Arc;

pub use dictation::{DictationEngine, DictationSpec};
pub use jobs::FileJobService;
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
    /// Live transcript while dictating: `committed` never changes once
    /// emitted; `volatile` is the still-open window's best guess.
    DictationPartial {
        profile_id: String,
        committed: String,
        volatile: String,
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
    JobProgress {
        id: String,
        stage: String,
        pct: f32,
    },
    JobSegment {
        id: String,
        segment: speakly_engine_types::Segment,
    },
    JobDone {
        id: String,
        duration_ms: u64,
    },
    JobError {
        id: String,
        message: String,
    },
    JobCancelled {
        id: String,
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
    pub jobs: FileJobService,
    pub meetings: MeetingService,
}

impl Engine {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        let stt = SttService::spawn();
        let models = ModelService::new(Arc::clone(&sink));
        let jobs = FileJobService::new(stt.clone(), Arc::clone(&sink));
        let meetings = MeetingService::new(stt.clone(), Arc::clone(&sink));
        let dictation = DictationEngine::new(stt.clone(), sink);
        Self {
            stt,
            dictation,
            models,
            jobs,
            meetings,
        }
    }

    /// Open the default input for ~500 ms and close it again. Exists so the
    /// app can trigger the macOS microphone permission prompt from an explicit
    /// user action (onboarding) instead of mid-dictation.
    pub fn mic_probe(&self) -> Result<(), String> {
        if self.dictation.is_active() {
            return Err("dictation is in progress — try again after it finishes".into());
        }
        let capture = crate::audio::capture::CaptureService::spawn();
        let (tx, rx) = crossbeam_channel::unbounded();
        capture.start(tx)?;
        let t0 = std::time::Instant::now();
        while t0.elapsed() < std::time::Duration::from_millis(500) {
            // Drain so the channel never backs up; ignore contents.
            let _ = rx.recv_timeout(std::time::Duration::from_millis(100));
        }
        capture.stop();
        Ok(())
    }
}
