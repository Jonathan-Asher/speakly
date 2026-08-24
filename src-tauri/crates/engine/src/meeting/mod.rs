//! Meeting capture: system audio via the ScreenCaptureKit sidecar, optionally
//! mixed with the microphone, transcribed live in fixed 15 s windows (VAD-gated
//! segmentation replaces the fixed windows later). One session at a time.

pub mod protocol;
pub mod sidecar;

use std::io::Write;
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::unbounded;

use crate::audio::capture::CaptureService;
use crate::audio::resample::resample_to_16k;
use crate::stt::{scaled_audio_ctx, DecodeRequest, SttService};
use crate::{EngineEvent, EventSink};

pub const WINDOW_MS: u64 = 15_000;
const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct MeetingOpts {
    pub sidecar_path: String,
    pub bundle_ids: Vec<String>,
    pub system: bool,
    pub mic: bool,
    pub model_id: String,
    pub model_path: String,
    pub language: String,
    pub scale_audio_ctx: bool,
}

struct ActiveSession {
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    stdin: Arc<Mutex<ChildStdin>>,
    /// Kept so dropping it (session end) closes the capture thread's channel.
    _mic: Option<CaptureService>,
}

pub struct MeetingService {
    stt: SttService,
    sink: Arc<dyn EventSink>,
    active: Mutex<Option<ActiveSession>>,
    next_id: AtomicU64,
}

impl MeetingService {
    pub fn new(stt: SttService, sink: Arc<dyn EventSink>) -> Self {
        Self {
            stt,
            sink,
            active: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }

    pub fn start(&self, opts: MeetingOpts) -> Result<u64, String> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return Err("a meeting capture is already running".into());
        }

        let session_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let system_buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));

        let proc = sidecar::spawn_capture(
            &opts.sidecar_path,
            &opts.bundle_ids,
            opts.system,
            TARGET_RATE,
        )?;
        let stdin = Arc::new(Mutex::new(proc.stdin));

        self.sink.emit(EngineEvent::MeetingStatus {
            session_id,
            state: "starting".into(),
            message: None,
        });

        // Optional microphone leg: its own capture service + collector, kept at
        // native rate and resampled at window close.
        let mut mic_service = None;
        let mic_state: Arc<Mutex<(Vec<f32>, u32)>> = Arc::new(Mutex::new((Vec::new(), 0)));
        if opts.mic {
            let capture = CaptureService::spawn();
            let (tx, rx) = unbounded::<Vec<f32>>();
            match capture.start(tx) {
                Ok(rate) => {
                    mic_state.lock().unwrap().1 = rate;
                    let collector = Arc::clone(&mic_state);
                    std::thread::Builder::new()
                        .name("speakly-meet-mic".into())
                        .spawn(move || {
                            while let Ok(chunk) = rx.recv() {
                                collector.lock().unwrap().0.extend_from_slice(&chunk);
                            }
                        })
                        .expect("spawn mic collector");
                    mic_service = Some(capture);
                }
                Err(e) => self.sink.emit(EngineEvent::Warning {
                    code: "mic".into(),
                    message: format!("meeting mic unavailable: {e}"),
                }),
            }
        }

        // Supervisor: owns the child, pumps frames into the system buffer.
        {
            let sink = Arc::clone(&self.sink);
            let stop = Arc::clone(&stop_flag);
            let buf = Arc::clone(&system_buf);
            let mut child = proc.child;
            let mut stdout = proc.stdout;
            std::thread::Builder::new()
                .name("speakly-meet-sidecar".into())
                .spawn(move || {
                    loop {
                        match protocol::read_frame(&mut stdout) {
                            Ok(Some(protocol::Frame::Audio { samples, .. })) => {
                                buf.lock().unwrap().extend_from_slice(&samples);
                            }
                            Ok(Some(protocol::Frame::Status(s))) => {
                                if s.contains("\"started\"") {
                                    sink.emit(EngineEvent::MeetingStatus {
                                        session_id,
                                        state: "live".into(),
                                        message: None,
                                    });
                                }
                            }
                            Ok(Some(protocol::Frame::Error(e))) => {
                                tracing::warn!("sidecar error: {e}");
                                if !stop.load(Ordering::Relaxed) {
                                    sink.emit(EngineEvent::MeetingStatus {
                                        session_id,
                                        state: "error".into(),
                                        message: Some(e),
                                    });
                                    stop.store(true, Ordering::Relaxed);
                                }
                                break;
                            }
                            Ok(None) | Err(_) => break,
                        }
                        if stop.load(Ordering::Relaxed) {
                            // Keep draining briefly; the stop command makes the
                            // sidecar exit, which lands us in EOF above.
                        }
                    }
                    // Reap, escalating to kill if the exit lingers.
                    let deadline = Instant::now() + Duration::from_millis(700);
                    while Instant::now() < deadline {
                        if matches!(child.try_wait(), Ok(Some(_))) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                })
                .expect("spawn sidecar supervisor");
        }

        // Ticker: closes 15 s windows, decodes, accumulates the transcript,
        // and finishes the session.
        {
            let sink = Arc::clone(&self.sink);
            let stop = Arc::clone(&stop_flag);
            let stt = self.stt.clone();
            let sys = Arc::clone(&system_buf);
            let mic = Arc::clone(&mic_state);
            let opts = opts.clone();
            std::thread::Builder::new()
                .name("speakly-meet-ticker".into())
                .spawn(move || {
                    let started = Instant::now();
                    let mut window_index: u64 = 0;
                    let mut transcript: Vec<String> = Vec::new();

                    loop {
                        let stopping = stop.load(Ordering::Relaxed);
                        let elapsed_ms = started.elapsed().as_millis() as u64;
                        let boundary = (window_index + 1) * WINDOW_MS;
                        if stopping || elapsed_ms >= boundary {
                            let sys_chunk = std::mem::take(&mut *sys.lock().unwrap());
                            let (mic_chunk, mic_rate) = {
                                let mut g = mic.lock().unwrap();
                                (std::mem::take(&mut g.0), g.1)
                            };
                            let mic_16k = if mic_rate > 0 {
                                resample_to_16k(&mic_chunk, mic_rate)
                            } else {
                                Vec::new()
                            };
                            let mix = mix_clamped(&sys_chunk, &mic_16k);
                            let (t0, mut t1) = window_bounds_ms(window_index, WINDOW_MS);
                            if stopping {
                                t1 = elapsed_ms.max(t0);
                            }
                            if mix.len() >= TARGET_RATE as usize {
                                let audio_ctx =
                                    opts.scale_audio_ctx.then(|| scaled_audio_ctx(mix.len()));
                                match stt.decode(DecodeRequest {
                                    model_id: opts.model_id.clone(),
                                    model_path: opts.model_path.clone(),
                                    language: opts.language.clone(),
                                    audio: mix,
                                    audio_ctx,
                                }) {
                                    Ok(out) if !out.text.is_empty() => {
                                        transcript.push(out.text.clone());
                                        sink.emit(EngineEvent::MeetingSegment {
                                            session_id,
                                            t0_ms: t0,
                                            t1_ms: t1,
                                            text: out.text,
                                            source: "mix".into(),
                                        });
                                    }
                                    Ok(_) => {}
                                    Err(e) => sink.emit(EngineEvent::Warning {
                                        code: "meeting_decode".into(),
                                        message: e,
                                    }),
                                }
                            }
                            window_index += 1;
                            if stopping {
                                sink.emit(EngineEvent::MeetingStatus {
                                    session_id,
                                    state: "stopped".into(),
                                    message: None,
                                });
                                sink.emit(EngineEvent::MeetingFinished {
                                    session_id,
                                    text: transcript.join("\n"),
                                    duration_ms: elapsed_ms,
                                });
                                return;
                            }
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                })
                .expect("spawn meeting ticker");
        }

        *active = Some(ActiveSession {
            session_id,
            stop_flag,
            stdin,
            _mic: mic_service,
        });
        Ok(session_id)
    }

    pub fn stop(&self, session_id: u64) -> Result<(), String> {
        let mut active = self.active.lock().unwrap();
        match active.as_ref() {
            Some(s) if s.session_id == session_id => {}
            Some(_) => return Err("unknown meeting session".into()),
            None => return Err("no meeting is running".into()),
        }
        let session = active.take().unwrap();
        session.stop_flag.store(true, Ordering::Relaxed);
        if let Ok(mut stdin) = session.stdin.lock() {
            let _ = stdin.write_all(b"{\"cmd\":\"stop\"}\n");
            let _ = stdin.flush();
        }
        // Dropping the session drops the mic CaptureService, ending its thread.
        Ok(())
    }
}

/// Sum two mono tracks sample-wise, padding the shorter with silence and
/// clamping into [-1, 1].
pub fn mix_clamped(a: &[f32], b: &[f32]) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| {
            let s = a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0);
            s.clamp(-1.0, 1.0)
        })
        .collect()
}

/// Wall-clock bounds of window `index` in session-relative milliseconds.
pub fn window_bounds_ms(index: u64, window_ms: u64) -> (u64, u64) {
    (index * window_ms, (index + 1) * window_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_pads_and_clamps() {
        let out = mix_clamped(&[0.8, -0.9, 0.1], &[0.5, -0.5]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], -1.0);
        assert!((out[2] - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn window_bounds_are_contiguous() {
        assert_eq!(window_bounds_ms(0, WINDOW_MS), (0, 15_000));
        assert_eq!(window_bounds_ms(2, WINDOW_MS), (30_000, 45_000));
        let (a1, b1) = window_bounds_ms(3, WINDOW_MS);
        let (a2, _) = window_bounds_ms(4, WINDOW_MS);
        assert_eq!(b1, a2);
        assert!(a1 < b1);
    }
}
