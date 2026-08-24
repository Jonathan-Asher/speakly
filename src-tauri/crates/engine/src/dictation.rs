//! Push-to-talk orchestration: arm capture on key-down, accumulate native-rate
//! mono audio, and on key-up resample → decode → emit the transcript. The app
//! layer owns what happens to the text (translate, paste, persist).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam_channel::unbounded;

use crate::audio::capture::CaptureService;
use crate::audio::resample::resample_to_16k;
use crate::stt::{scaled_audio_ctx, DecodeRequest, SttService};
use crate::{EngineEvent, EventSink, Phase};

/// Everything the engine needs to run one dictation, resolved by the app layer
/// from the active profile + settings.
#[derive(Clone)]
pub struct DictationSpec {
    pub profile_id: String,
    pub language: String,
    pub model_id: String,
    pub model_path: String,
    /// Scale down the encoder context for speed (validated per model).
    pub scale_audio_ctx: bool,
}

const MIN_UTTERANCE_SECS: f32 = 0.4;

struct Active {
    spec: DictationSpec,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    started: Instant,
}

pub struct DictationEngine {
    capture: CaptureService,
    stt: SttService,
    sink: Arc<dyn EventSink>,
    active: Mutex<Option<Active>>,
}

impl DictationEngine {
    pub fn new(stt: SttService, sink: Arc<dyn EventSink>) -> Self {
        Self {
            capture: CaptureService::spawn(),
            stt,
            sink,
            active: Mutex::new(None),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }

    pub fn start(&self, spec: DictationSpec) {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return;
        }

        let (tx, rx) = unbounded::<Vec<f32>>();
        let sample_rate = match self.capture.start(tx) {
            Ok(rate) => rate,
            Err(e) => {
                self.sink.emit(EngineEvent::Warning {
                    code: "mic".into(),
                    message: e,
                });
                self.sink.emit(EngineEvent::DictationState {
                    phase: Phase::Error,
                    profile_id: spec.profile_id.clone(),
                });
                return;
            }
        };

        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
        let collector_buf = Arc::clone(&buffer);
        std::thread::Builder::new()
            .name("speakly-collect".into())
            .spawn(move || {
                // Ends when capture stops and the stream (with its sender) drops.
                while let Ok(chunk) = rx.recv() {
                    collector_buf.lock().unwrap().extend_from_slice(&chunk);
                }
            })
            .expect("spawn collector");

        self.sink.emit(EngineEvent::DictationState {
            phase: Phase::Listening,
            profile_id: spec.profile_id.clone(),
        });
        *active = Some(Active {
            spec,
            buffer,
            sample_rate,
            started: Instant::now(),
        });
    }

    /// Key-up: stop capture and transcribe on a worker thread. Emits
    /// `TranscriptReady` (or a warning + `Idle`) when done.
    pub fn stop(&self) {
        let Some(active) = self.active.lock().unwrap().take() else {
            return;
        };
        self.capture.stop();

        let sink = Arc::clone(&self.sink);
        let stt = self.stt.clone();
        std::thread::Builder::new()
            .name("speakly-finalize".into())
            .spawn(move || finalize(active, stt, sink))
            .expect("spawn finalize");
    }

    pub fn cancel(&self) {
        let Some(active) = self.active.lock().unwrap().take() else {
            return;
        };
        self.capture.stop();
        self.sink.emit(EngineEvent::DictationState {
            phase: Phase::Idle,
            profile_id: active.spec.profile_id,
        });
    }
}

fn finalize(active: Active, stt: SttService, sink: Arc<dyn EventSink>) {
    let Active {
        spec,
        buffer,
        sample_rate,
        started,
    } = active;
    let key_up = Instant::now();

    sink.emit(EngineEvent::DictationState {
        phase: Phase::Transcribing,
        profile_id: spec.profile_id.clone(),
    });

    // Give the tail of the capture callback queue a moment to drain.
    std::thread::sleep(std::time::Duration::from_millis(60));
    let native = std::mem::take(&mut *buffer.lock().unwrap());

    let secs = native.len() as f32 / sample_rate as f32;
    if secs < MIN_UTTERANCE_SECS {
        sink.emit(EngineEvent::Warning {
            code: "too_short".into(),
            message: "Press and hold while speaking".into(),
        });
        sink.emit(EngineEvent::DictationState {
            phase: Phase::Idle,
            profile_id: spec.profile_id,
        });
        return;
    }

    let audio = resample_to_16k(&native, sample_rate);
    let audio_ctx = spec.scale_audio_ctx.then(|| scaled_audio_ctx(audio.len()));

    let result = stt.decode(DecodeRequest {
        model_id: spec.model_id.clone(),
        model_path: spec.model_path.clone(),
        language: spec.language.clone(),
        audio,
        audio_ctx,
    });

    match result {
        Ok(outcome) if outcome.text.is_empty() => {
            sink.emit(EngineEvent::Warning {
                code: "no_speech".into(),
                message: "No speech detected".into(),
            });
            sink.emit(EngineEvent::DictationState {
                phase: Phase::Idle,
                profile_id: spec.profile_id,
            });
        }
        Ok(outcome) => {
            sink.emit(EngineEvent::TranscriptReady {
                profile_id: spec.profile_id,
                text: outcome.text,
                utterance_ms: (key_up - started).as_millis() as u64,
                decode_ms: outcome.decode_ms,
                latency_ms: key_up.elapsed().as_millis() as u64,
            });
        }
        Err(e) => {
            sink.emit(EngineEvent::Warning {
                code: "decode".into(),
                message: e,
            });
            sink.emit(EngineEvent::DictationState {
                phase: Phase::Error,
                profile_id: spec.profile_id,
            });
        }
    }
}
