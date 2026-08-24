//! Speakly engine: audio capture, VAD, speech-to-text inference, scheduling,
//! model management, diarization, and meeting capture. UI-free — the Tauri app
//! crate drives it and receives events through [`EventSink`].

pub mod stt;

/// The engine's only way to talk to the outside world. The app crate
/// implements this over `AppHandle::emit`; tests and CLI spikes implement it
/// over a channel or stdout.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: EngineEvent);
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Warning { code: String, message: String },
}
