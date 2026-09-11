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
    /// Microphones to record from, best first; empty means the OS default.
    pub mic_priority: Vec<String>,
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

/// What arrived while a session's microphone was still opening, to be applied
/// the moment it goes live. Opening can take seconds — an iPhone reached over
/// Continuity is the worst case — and a key release, a growing combination or
/// an Esc can all land inside that window.
#[derive(Default)]
struct Pending {
    retarget: Option<DictationSpec>,
    end: Option<End>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum End {
    Stop,
    Cancel,
}

impl Pending {
    /// Cancel outranks a stop already recorded: Esc means discard the
    /// recording, whatever the key release asked for a moment earlier.
    fn record_end(&mut self, end: End) {
        if self.end.is_none() || end == End::Cancel {
            self.end = Some(end);
        }
    }
}

pub struct DictationEngine {
    capture: CaptureService,
    stt: SttService,
    sink: Arc<dyn EventSink>,
    active: Mutex<Option<Active>>,
    /// Profile of a session whose microphone is still opening. `active` is
    /// still `None` then, but a session is on its way and every caller must
    /// treat it as one.
    ///
    /// Lock order is `active` → `starting` → `pending`; never the reverse.
    starting: Mutex<Option<String>>,
    pending: Mutex<Pending>,
}

impl DictationEngine {
    pub fn new(stt: SttService, sink: Arc<dyn EventSink>) -> Self {
        Self {
            capture: CaptureService::spawn(),
            stt,
            sink,
            active: Mutex::new(None),
            starting: Mutex::new(None),
            pending: Mutex::new(Pending::default()),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.lock().unwrap().is_some() || self.starting.lock().unwrap().is_some()
    }

    /// Profile of the running session, or of one whose microphone is still
    /// opening — a session being born counts, or the key release that ends it
    /// would be dropped for having nothing to stop.
    pub fn active_profile_id(&self) -> Option<String> {
        let live = self
            .active
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.spec.lock().unwrap().profile_id.clone());
        live.or_else(|| self.starting.lock().unwrap().clone())
    }

    /// Swap the running session's profile in place — the combination evolved
    /// (e.g. a held ⌥ grew into ⌥Space). Audio keeps recording; partials use
    /// the new spec from the next tick; the final decode uses it outright.
    /// Returns false when no session is active.
    pub fn retarget(&self, spec: DictationSpec) -> bool {
        let active = self.active.lock().unwrap();
        let Some(active) = active.as_ref() else {
            drop(active);
            // The combination grew while the microphone was still opening;
            // the starter applies this as soon as there is a session.
            if self.starting.lock().unwrap().is_some() {
                self.pending.lock().unwrap().retarget = Some(spec);
                return true;
            }
            return false;
        };
        let profile_id = spec.profile_id.clone();
        *active.spec.lock().unwrap() = spec;
        {
            // Whatever the streaming partials already committed was decoded
            // with the previous profile's model and language — keeping it would
            // paste Hebrew for speech the user wanted transcribed in English.
            // Drop it so the final decode re-runs the whole utterance under the
            // new profile, and discard any partial still in flight.
            let mut shared = active.shared.lock().unwrap();
            shared.state.reset_transcript();
            if let Some(stale) = shared.pending_stale.take() {
                stale.store(true, Ordering::Relaxed);
            }
        }
        self.sink.emit(EngineEvent::DictationState {
            phase: Phase::Listening,
            profile_id,
        });
        true
    }

    pub fn start(&self, spec: DictationSpec) {
        // Claim the slot, then let go of every lock before touching the
        // microphone. Opening one blocks for as long as the device takes —
        // seconds, for an iPhone over Continuity — and holding `active`
        // across that stalls is_active/stop/cancel/retarget with it. Those
        // are called straight from the event-tap callback, and macOS disables
        // a tap that stops responding, so the key release never arrives and
        // the recording runs on as if it were a toggle.
        {
            let active = self.active.lock().unwrap();
            let mut starting = self.starting.lock().unwrap();
            if active.is_some() || starting.is_some() {
                return;
            }
            *starting = Some(spec.profile_id.clone());
            *self.pending.lock().unwrap() = Pending::default();
        }

        let (tx, rx) = unbounded::<Vec<f32>>();
        let sample_rate = match self.capture.start(tx, spec.mic_priority.clone()) {
            Ok(rate) => rate,
            Err(e) => {
                *self.starting.lock().unwrap() = None;
                let message = if e.contains("no input device") {
                    "No microphone found — connect one and try again".to_string()
                } else {
                    format!(
                        "Couldn't open the microphone ({e}). If access was denied, enable \
                         Speakly under System Settings → Privacy & Security → Microphone."
                    )
                };
                self.sink.emit(EngineEvent::Warning {
                    code: "mic".into(),
                    message,
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
        // Re-acquire only now that the device is open and everything is built.
        *self.active.lock().unwrap() = Some(Active {
            spec,
            buffer,
            sample_rate,
            started: Instant::now(),
            ticker_stop,
            shared,
            ticker: Some(ticker),
        });
        *self.starting.lock().unwrap() = None;

        // Honour whatever landed while the microphone was opening, in the
        // order it would have taken effect.
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        if let Some(spec) = pending.retarget {
            self.retarget(spec);
        }
        match pending.end {
            Some(End::Stop) => self.stop(),
            Some(End::Cancel) => self.cancel(),
            None => {}
        }
    }

    /// Key-up: stop capture and transcribe on a worker thread. Emits
    /// `TranscriptReady` (or a warning + `Idle`) when done.
    pub fn stop(&self) {
        let Some(active) = self.active.lock().unwrap().take() else {
            self.defer_end(End::Stop);
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
            self.defer_end(End::Cancel);
            return;
        };
        self.capture.stop();
        signal_ticker(&active);
        self.sink.emit(EngineEvent::DictationState {
            phase: Phase::Cancelled,
            profile_id: active.spec.lock().unwrap().profile_id.clone(),
        });
        // The ticker holds only Arcs; it exits on its own after the signal.
    }

    /// Record an end requested before the session went live, so the starter
    /// can apply it. A no-op when nothing is starting either — then there was
    /// genuinely nothing to end.
    fn defer_end(&self, end: End) {
        if self.starting.lock().unwrap().is_none() {
            return;
        }
        self.pending.lock().unwrap().record_end(end);
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
                    let span_s = (boundary - offset) as f32 / 16_000.0;
                    if sh.state.commit(&out.text, boundary) {
                        tracing::debug!("committed {span_s:.1}s → {} chars", out.text.len());
                    } else {
                        // Left uncommitted on purpose — the final pass retries it.
                        tracing::warn!("closed span of {span_s:.1}s decoded to nothing; keeping its audio for the final pass");
                    }
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
    // The live pass is for the on-screen preview ONLY. Its chunk-sized decodes
    // are far worse than one pass over the whole recording — whisper has almost
    // no context in a two-second window, and measurements showed those chunks
    // yielding a couple of characters where a single pass produced full
    // sentences. So the pasted text always comes from one decode of everything.
    // It is nearly free: a minute of audio decodes in about a second here.
    let preview_secs = state.committed_offset().min(audio.len()) as f32 / 16_000.0;
    let preview_chars = state.committed_text().chars().count();
    let tail: &[f32] = &audio;

    // Trim leading/trailing silence (kills key-press noise and
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
            let decoded = tail_text.trim().to_string();
            // Only fall back to the preview if the full decode produced nothing
            // — pasting a rough preview beats pasting nothing at all.
            let full = if decoded.is_empty() && preview_chars > 0 {
                tracing::warn!("full decode came back empty; falling back to the live preview");
                state.committed_text().to_string()
            } else {
                decoded
            };
            tracing::info!(
                "dictation finalised: {:.1}s audio | preview {:.1}s/{} chars | decoded {:.1}s → {} chars | decode {} ms",
                audio.len() as f32 / 16_000.0,
                preview_secs,
                preview_chars,
                tail.len() as f32 / 16_000.0,
                full.chars().count(),
                decode_ms
            );
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

#[cfg(test)]
mod tests {
    use super::{End, Pending};

    /// A key release during a slow microphone open records a stop, so the
    /// starter can apply it the moment there is a session to apply it to.
    /// Without this the release is dropped and the recording runs on until
    /// the next press, which is what made an iPhone over Continuity behave
    /// as though hold-to-talk were a toggle.
    #[test]
    fn a_release_during_the_open_is_remembered() {
        let mut pending = Pending::default();
        assert_eq!(pending.end, None);
        pending.record_end(End::Stop);
        assert_eq!(pending.end, Some(End::Stop));
    }

    #[test]
    fn escape_outranks_a_release_already_recorded() {
        let mut pending = Pending::default();
        pending.record_end(End::Stop);
        pending.record_end(End::Cancel);
        assert_eq!(
            pending.end,
            Some(End::Cancel),
            "Esc must discard, not paste"
        );
    }

    #[test]
    fn a_release_does_not_downgrade_an_escape() {
        let mut pending = Pending::default();
        pending.record_end(End::Cancel);
        pending.record_end(End::Stop);
        assert_eq!(pending.end, Some(End::Cancel));
    }
}
