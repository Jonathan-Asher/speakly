//! Offline speaker diarization: pyannote segmentation-3.0 plus a speaker
//! embedding model and clustering, all ONNX via sherpa-rs (no Python).
//! CPU-bound; runs on the calling worker thread — file-job or meeting
//! post-processing — never the Metal STT lane.

pub mod merge;

use std::path::Path;

use sherpa_rs::diarize::{Diarize, DiarizeConfig};

/// Speaker identification options; model paths are resolved by the app layer.
#[derive(Debug, Clone)]
pub struct DiarizeOpts {
    pub seg_model_path: String,
    pub emb_model_path: String,
    pub num_speakers: Option<usize>,
}

/// One who-spoke-when span, session/file-relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerTurn {
    pub t0_ms: u64,
    pub t1_ms: u64,
    /// 0-based cluster index; presentation maps it to "Speaker N".
    pub speaker: u32,
}

/// Diarize 16 kHz mono audio. `num_speakers` fixes the cluster count; `None`
/// lets threshold clustering pick it. Expect roughly 0.3–1.0× audio duration
/// on Apple-Silicon CPU and a few hundred MB of working memory.
pub fn diarize(
    audio_16k: &[f32],
    seg_model: &Path,
    emb_model: &Path,
    num_speakers: Option<usize>,
) -> Result<Vec<SpeakerTurn>, String> {
    if !seg_model.is_file() {
        return Err(format!("segmentation model missing: {}", seg_model.display()));
    }
    if !emb_model.is_file() {
        return Err(format!("embedding model missing: {}", emb_model.display()));
    }

    let config = DiarizeConfig {
        // <= 0 means "not fixed" — sherpa-onnx then clusters by threshold.
        num_clusters: Some(num_speakers.map(|n| n as i32).unwrap_or(-1)),
        threshold: Some(0.5),
        // Suppress sub-300 ms blips and merge pauses under 500 ms.
        min_duration_on: Some(0.3),
        min_duration_off: Some(0.5),
        provider: None,
        debug: false,
    };
    let mut engine = Diarize::new(seg_model, emb_model, config)
        .map_err(|e| format!("diarization init: {e}"))?;
    let turns = engine
        .compute(audio_16k.to_vec(), None)
        .map_err(|e| format!("diarization: {e}"))?;

    Ok(turns
        .into_iter()
        .filter(|t| t.speaker >= 0 && t.end > t.start)
        .map(|t| SpeakerTurn {
            t0_ms: (t.start as f64 * 1000.0) as u64,
            t1_ms: (t.end as f64 * 1000.0) as u64,
            speaker: t.speaker as u32,
        })
        .collect())
}
