//! Push-to-talk orchestration: arm capture on key-down, accumulate native-rate
//! mono audio, stream live partial transcripts while speaking, and on key-up
//! decode only the uncommitted tail. The app layer owns what happens to the
//! text (translate, paste, persist).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::unbounded;

use crate::audio::capture::CaptureService;
use crate::audio::resample::resample_to_16k;
use crate::stt::session::SessionState;
use crate::stt::{scaled_audio_ctx, DecodeRequest, SttService};
use crate::vad::{speech_bounds, SileroVad, VadEngine};
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
    /// Silero ggml file for live segmentation + silence trimming; `None`
    /// (not yet downloaded) degrades gracefully to final-only behavior.
    pub vad_model_path: Option<String>,
}

const MIN_UTTERANCE_SECS: f32 = 0.4;
/// Don't bother decoding a partial window shorter than this.
const MIN_PARTIAL_SECS: f32 = 0.7;
/// Committed boundaries shorter than this stay volatile.
const MIN_COMMIT_SECS: f32 = 0.5;

/// Ticker/finalize shared state; the ticker exits before finalize touches it.
struct SessionShared {
    state: SessionState,
    vad: Option<SileroVad>,
    /// Stale flag of the most recently queued volatile decode.
    pending_stale: Option<Arc<AtomicBool>>,
}

struct Active {
    spec: Arc<Mutex<DictationSpec>>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    started: Instant,
    ticker_stop: Arc<AtomicBool>,
    shared: Arc<Mutex<SessionShared>>,
    ticker: Option<JoinHandle<()>>,
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

    /// Profile of the running session, if any.
    pub fn active_profile_id(&self) -> Option<String> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.spec.lock().unwrap().profile_id.clone())
    }

    /// Swap the running session's profile in place — the combination evolved
    /// (e.g. a held ⌥ grew into ⌥Space). Audio keeps recording; partials use
    /// the new spec from the next tick; the final decode uses it outright.
    /// Returns false when no session is active.
    pub fn retarget(&self, spec: DictationSpec) -> bool {
        let active = self.active.lock().unwrap();
        let Some(active) = active.as_ref() else {
            return false;
        };
        let profile_id = spec.profile_id.clone();
        *active.spec.lock().unwrap() = spec;
        self.sink.emit(EngineEvent::DictationState {
            phase: Phase::Listening,
            profile_id,
        });
        true
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

        let ticker_stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(SessionShared {
            state: SessionState::new(),
            vad: None,
            pending_stale: None,
        }));
        let spec = Arc::new(Mutex::new(spec));
        let ticker = {
            let spec = Arc::clone(&spec);
            let buffer = Arc::clone(&buffer);
            let stop = Arc::clone(&ticker_stop);
            let shared = Arc::clone(&shared);
            let stt = self.stt.clone();
            let sink = Arc::clone(&self.sink);
            std::thread::Builder::new()
                .name("speakly-partials".into())
                .spawn(move || ticker_loop(spec, buffer, sample_rate, stop, shared, stt, sink))
                .expect("spawn partial ticker")
        };

        self.sink.emit(EngineEvent::DictationState {
            phase: Phase::Listening,
            profile_id: spec.lock().unwrap().profile_id.clone(),
        });
        *active = Some(Active {
            spec,
            buffer,
            sample_rate,
            started: Instant::now(),
            ticker_stop,
            shared,
            ticker: Some(ticker),
        });
    }

    /// Key-up: stop capture and transcribe on a worker thread. Emits
    /// `TranscriptReady` (or a warning + `Idle`) when done.
    pub fn stop(&self) {
        let Some(active) = self.active.lock().unwrap().take() else {
            return;
        };
        self.capture.stop();
        signal_ticker(&active);

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
        signal_ticker(&active);
        self.sink.emit(EngineEvent::DictationState {
            phase: Phase::Idle,
            profile_id: active.spec.lock().unwrap().profile_id.clone(),
        });
        // The ticker holds only Arcs; it exits on its own after the signal.
    }
}

/// Flag the in-queue partial as superseded and tell the ticker to wind down.
fn signal_ticker(active: &Active) {
    if let Some(stale) = active.shared.lock().unwrap().pending_stale.take() {
        stale.store(true, Ordering::Relaxed);
    }
    active.ticker_stop.store(true, Ordering::Relaxed);
}

fn decode_window(
    stt: &SttService,
    spec: &DictationSpec,
    audio: &[f32],
    stale: Option<Arc<AtomicBool>>,
) -> Result<crate::stt::DecodeOutcome, String> {
    let audio_ctx = spec.scale_audio_ctx.then(|| scaled_audio_ctx(audio.len()));
    stt.decode(DecodeRequest {
        model_id: spec.model_id.clone(),
        model_path: spec.model_path.clone(),
        language: spec.language.clone(),
        audio: audio.to_vec(),
        audio_ctx,
        with_timestamps: false,
        drop_if_stale: stale,
    })
}

/// Streaming partials: every tick, commit any VAD-closed speech and decode the
/// open tail as volatile text. Committed text never changes afterwards.
fn ticker_loop(
    spec: Arc<Mutex<DictationSpec>>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<SessionShared>>,
    stt: SttService,
    sink: Arc<dyn EventSink>,
) {
    let vad_path = spec.lock().unwrap().vad_model_path.clone();
    if let Some(path) = &vad_path {
        match SileroVad::load(path) {
            Ok(vad) => shared.lock().unwrap().vad = Some(vad),
            Err(e) => tracing::warn!("dictation VAD unavailable: {e}"),
        }
    }

    let min_partial = (MIN_PARTIAL_SECS * 16_000.0) as usize;
    let min_commit = (MIN_COMMIT_SECS * 16_000.0) as usize;
    let mut last_tick = Instant::now();

    loop {
        std::thread::sleep(Duration::from_millis(50));
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let (tick_ms, disabled) = {
            let sh = shared.lock().unwrap();
            (sh.state.tick_ms, sh.state.partials_disabled)
        };
        if disabled || last_tick.elapsed() < Duration::from_millis(tick_ms) {
            continue;
        }
        last_tick = Instant::now();
        // Fresh snapshot: a retarget (combination change) applies from the
        // next tick onward.
        let tick_spec = spec.lock().unwrap().clone();

        let native = buffer.lock().unwrap().clone();
        if (native.len() as f32 / sample_rate as f32) < MIN_PARTIAL_SECS {
            continue;
        }
        let audio = resample_to_16k(&native, sample_rate);

        // Find a commit boundary in the open window via VAD.
        let (offset, boundary) = {
            let mut sh = shared.lock().unwrap();
            let offset = sh.state.committed_offset().min(audio.len());
            let window = &audio[offset..];
            let boundary = sh
                .vad
                .as_mut()
                .and_then(|vad| vad.analyze(window).ok())
                .and_then(|analysis| analysis.closed.last().map(|s| offset + s.end));
            (offset, boundary)
        };
        if stop.load(Ordering::Relaxed) {
            return;
        }

        // Commit the closed span (its text is now immutable).
        if let Some(boundary) = boundary.filter(|b| *b > offset + min_commit) {
            match decode_window(&stt, &tick_spec, &audio[offset..boundary], None) {
                Ok(out) => {
                    let mut sh = shared.lock().unwrap();
                    sh.state.apply_first_measurement(out.decode_ms);
                    sh.state.commit(&out.text, boundary);
                }
                Err(e) => tracing::debug!("commit decode failed: {e}"),
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }

        // Decode the open tail as volatile text.
        let offset = shared
            .lock()
            .unwrap()
            .state
            .committed_offset()
            .min(audio.len());
        let tail = &audio[offset..];
        let mut volatile = String::new();
        if tail.len() >= min_partial {
            let stale = Arc::new(AtomicBool::new(false));
            shared.lock().unwrap().pending_stale = Some(Arc::clone(&stale));
            match decode_window(&stt, &tick_spec, tail, Some(stale)) {
                Ok(out) => {
                    let mut sh = shared.lock().unwrap();
                    if let Some(decision) = sh.state.apply_first_measurement(out.decode_ms) {
                        tracing::info!(
                            "first partial decode {} ms → {:?}",
                            out.decode_ms,
                            decision
                        );
                    }
                    volatile = out.text;
                }
                Err(e) if e == "stale" => continue,
                Err(e) => tracing::debug!("partial decode failed: {e}"),
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }

        let committed = shared.lock().unwrap().state.committed_text().to_string();
        if !committed.is_empty() || !volatile.is_empty() {
            sink.emit(EngineEvent::DictationPartial {
                profile_id: tick_spec.profile_id.clone(),
                committed,
                volatile,
            });
        }
    }
}

fn finalize(mut active: Active, stt: SttService, sink: Arc<dyn EventSink>) {
    let key_up = Instant::now();

    sink.emit(EngineEvent::DictationState {
        phase: Phase::Transcribing,
        profile_id: active.spec.lock().unwrap().profile_id.clone(),
    });

    // Give the tail of the capture callback queue a moment to drain, and let
    // the ticker finish its in-flight step (queued partials are stale-dropped).
    std::thread::sleep(Duration::from_millis(60));
    if let Some(handle) = active.ticker.take() {
        let _ = handle.join();
    }

    let Active {
        spec: shared_spec,
        buffer,
        sample_rate,
        started,
        shared,
        ..
    } = active;
    // Snapshot: the combination is settled by key-up; the final profile wins.
    let spec = shared_spec.lock().unwrap().clone();
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
    let (state, mut vad) = {
        let mut sh = shared.lock().unwrap();
        (std::mem::take(&mut sh.state), sh.vad.take())
    };
    let offset = state.committed_offset().min(audio.len());
    let tail: &[f32] = &audio[offset..];

    // Trim leading/trailing silence off the tail (kills key-press noise and
    // silence-hallucinations); pure silence skips the decode entirely.
    let mut trimmed: Option<Vec<f32>> = None;
    let mut tail_is_silence = false;
    if let Some(vad) = vad.as_mut() {
        if let Ok(analysis) = vad.analyze(tail) {
            match speech_bounds(&analysis, tail.len(), 150) {
                Some((s, e)) if e > s => trimmed = Some(tail[s..e].to_vec()),
                _ => tail_is_silence = true,
            }
        }
    }
    let tail: &[f32] = trimmed.as_deref().unwrap_or(tail);

    let min_tail = (0.25 * 16_000.0) as usize;
    let tail_text = if tail_is_silence || tail.len() < min_tail {
        Ok((String::new(), 0u64))
    } else {
        decode_window(&stt, &spec, tail, None).map(|o| (o.text, o.decode_ms))
    };

    match tail_text {
        Ok((tail_text, decode_ms)) => {
            let full = state.full_text(&tail_text);
            if full.is_empty() {
                sink.emit(EngineEvent::Warning {
                    code: "no_speech".into(),
                    message: "No speech detected".into(),
                });
                sink.emit(EngineEvent::DictationState {
                    phase: Phase::Idle,
                    profile_id: spec.profile_id,
                });
            } else {
                sink.emit(EngineEvent::TranscriptReady {
                    profile_id: spec.profile_id,
                    text: full,
                    utterance_ms: (key_up - started).as_millis() as u64,
                    decode_ms,
                    latency_ms: key_up.elapsed().as_millis() as u64,
                });
            }
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
