//! Warm whisper contexts on a single dedicated inference thread. All decodes
//! are serialized here (whisper is effectively single-lane per GPU); contexts
//! stay loaded between utterances, which is what makes dictation instant
//! (~0.6 s cold load vs ~0 warm, measured in docs/SPIKES.md).

use std::collections::HashMap;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct DecodeRequest {
    pub model_id: String,
    pub model_path: String,
    pub language: String,
    /// 16 kHz mono samples.
    pub audio: Vec<f32>,
    /// Encoder context override (`None` = full 1500). Scaled-down values are a
    /// ~3× speedup for short utterances; enable per model once validated.
    pub audio_ctx: Option<i32>,
}

pub struct DecodeOutcome {
    pub text: String,
    pub decode_ms: u64,
}

enum Job {
    Decode {
        req: DecodeRequest,
        reply: Sender<Result<DecodeOutcome, String>>,
    },
    Preload {
        model_id: String,
        model_path: String,
        reply: Sender<Result<u64, String>>,
    },
}

#[derive(Clone)]
pub struct SttService {
    job_tx: Sender<Job>,
}

impl SttService {
    pub fn spawn() -> Self {
        let (job_tx, job_rx) = unbounded::<Job>();
        std::thread::Builder::new()
            .name("speakly-stt".into())
            .spawn(move || stt_thread(job_rx))
            .expect("spawn stt thread");
        Self { job_tx }
    }

    /// Load a model into the warm pool without decoding. Returns load millis.
    pub fn preload(&self, model_id: &str, model_path: &str) -> Result<u64, String> {
        let (reply_tx, reply_rx) = bounded(1);
        self.job_tx
            .send(Job::Preload {
                model_id: model_id.into(),
                model_path: model_path.into(),
                reply: reply_tx,
            })
            .map_err(|_| "stt thread gone".to_string())?;
        reply_rx.recv().map_err(|_| "stt thread gone".to_string())?
    }

    /// Blocking decode; called from worker threads, never the main thread.
    pub fn decode(&self, req: DecodeRequest) -> Result<DecodeOutcome, String> {
        let (reply_tx, reply_rx) = bounded(1);
        self.job_tx
            .send(Job::Decode {
                req,
                reply: reply_tx,
            })
            .map_err(|_| "stt thread gone".to_string())?;
        reply_rx.recv().map_err(|_| "stt thread gone".to_string())?
    }
}

fn stt_thread(job_rx: Receiver<Job>) {
    let mut contexts: HashMap<String, WhisperContext> = HashMap::new();

    while let Ok(job) = job_rx.recv() {
        match job {
            Job::Preload {
                model_id,
                model_path,
                reply,
            } => {
                let _ = reply.send(ensure_context(&mut contexts, &model_id, &model_path));
            }
            Job::Decode { req, reply } => {
                let outcome = ensure_context(&mut contexts, &req.model_id, &req.model_path)
                    .and_then(|_| run_decode(&contexts[&req.model_id], &req));
                let _ = reply.send(outcome);
            }
        }
    }
}

fn ensure_context(
    contexts: &mut HashMap<String, WhisperContext>,
    model_id: &str,
    model_path: &str,
) -> Result<u64, String> {
    if contexts.contains_key(model_id) {
        return Ok(0);
    }
    if !std::path::Path::new(model_path).is_file() {
        return Err(format!("model file not found: {model_path}"));
    }
    let mut params = WhisperContextParameters::default();
    params.use_gpu(true);
    params.flash_attn(true);
    let t = std::time::Instant::now();
    let ctx = WhisperContext::new_with_params(model_path, params)
        .map_err(|e| format!("load model {model_id}: {e}"))?;
    let ms = t.elapsed().as_millis() as u64;
    tracing::info!("loaded model {model_id} in {ms} ms");
    contexts.insert(model_id.to_string(), ctx);
    Ok(ms)
}

fn run_decode(ctx: &WhisperContext, req: &DecodeRequest) -> Result<DecodeOutcome, String> {
    let mut state = ctx.create_state().map_err(|e| format!("state: {e}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(&req.language));
    params.set_n_threads(4);
    params.set_translate(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_nst(true);
    if let Some(ac) = req.audio_ctx {
        params.set_audio_ctx(ac);
    }

    let t = std::time::Instant::now();
    state
        .full(params, &req.audio)
        .map_err(|e| format!("decode: {e}"))?;
    let decode_ms = t.elapsed().as_millis() as u64;

    let mut text = String::new();
    for i in 0..state.full_n_segments() {
        if let Some(seg) = state.get_segment(i) {
            if let Ok(s) = seg.to_str_lossy() {
                text.push_str(&s);
            }
        }
    }
    Ok(DecodeOutcome {
        text: postprocess(&text),
        decode_ms,
    })
}

/// Minimal hallucination filtering: trim, and treat pure non-speech marker
/// output like "[BLANK_AUDIO]" or "(מוזיקה)" as empty.
fn postprocess(raw: &str) -> String {
    let text = raw.trim();
    let is_marker = |s: &str| {
        (s.starts_with('[') && s.ends_with(']')) || (s.starts_with('(') && s.ends_with(')'))
    };
    if !text.is_empty() && text.split_whitespace().all(is_marker) {
        return String::new();
    }
    text.to_string()
}

/// `ceil(len/30 s × 1500) + 128`, clamped to the full 1500 — the measured ~3×
/// short-utterance speedup (docs/SPIKES.md). Integer math: 1500 encoder
/// positions per 30 s of 16 kHz audio = one per 320 samples.
pub fn scaled_audio_ctx(n_samples_16k: usize) -> i32 {
    ((n_samples_16k.div_ceil(320)) as i32 + 128).min(1500)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postprocess_drops_marker_only_output() {
        assert_eq!(postprocess(" [BLANK_AUDIO] "), "");
        assert_eq!(postprocess("(מוזיקה)"), "");
        assert_eq!(postprocess(" שלום עולם "), "שלום עולם");
    }

    #[test]
    fn audio_ctx_scales_and_clamps() {
        assert_eq!(scaled_audio_ctx(16_000 * 8), 528);
        assert_eq!(scaled_audio_ctx(16_000 * 60), 1500);
    }
}
