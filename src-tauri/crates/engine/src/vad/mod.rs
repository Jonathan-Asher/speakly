//! Standalone Silero VAD (via whisper.cpp's ggml build of it) behind a small
//! trait so the gate logic stays testable without the native model. Used for
//! streaming-dictation commit boundaries and final-utterance silence trimming.

pub mod gate;

use whisper_rs::{WhisperVadContext, WhisperVadContextParams};

use gate::{GateConfig, GateOutput};

/// Sample-domain speech segment on 16 kHz audio: `[start, end)` in samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleSegment {
    pub start: usize,
    pub end: usize,
}

pub struct VadAnalysis {
    pub closed: Vec<SampleSegment>,
    pub open_start: Option<usize>,
}

pub trait VadEngine: Send {
    /// Run the gate over `samples` (16 kHz mono).
    fn analyze(&mut self, samples: &[f32]) -> Result<VadAnalysis, String>;
}

/// Silero v5 ggml through whisper-rs's `WhisperVadContext`.
pub struct SileroVad {
    ctx: WhisperVadContext,
}

impl SileroVad {
    pub fn load(model_path: &str) -> Result<Self, String> {
        let params = WhisperVadContextParams::new();
        let ctx = WhisperVadContext::new(model_path, params)
            .map_err(|e| format!("load VAD model: {e}"))?;
        Ok(Self { ctx })
    }
}

impl VadEngine for SileroVad {
    fn analyze(&mut self, samples: &[f32]) -> Result<VadAnalysis, String> {
        if samples.is_empty() {
            return Ok(VadAnalysis {
                closed: Vec::new(),
                open_start: None,
            });
        }
        self.ctx
            .detect_speech(samples)
            .map_err(|e| format!("vad: {e}"))?;
        let probs = self.ctx.probabilities();
        if probs.is_empty() {
            return Ok(VadAnalysis {
                closed: Vec::new(),
                open_start: None,
            });
        }
        // Frame size is derived, not assumed (Silero v5 at 16 kHz is 512
        // samples/frame, but let the backend own that detail).
        let frame_samples = (samples.len() / probs.len()).max(1);
        let frame_ms = (frame_samples as u32 * 1000) / 16_000;
        let cfg = GateConfig::dictation(frame_ms.max(1));
        let GateOutput { closed, open_start } = gate::run(&cfg, probs);
        Ok(VadAnalysis {
            closed: closed
                .into_iter()
                .map(|s| SampleSegment {
                    start: s.start * frame_samples,
                    end: (s.end * frame_samples).min(samples.len()),
                })
                .collect(),
            open_start: open_start.map(|f| f * frame_samples),
        })
    }
}

/// Trim bounds `[start, end)` around all detected speech, padded by
/// `pad_ms`. `None` means no speech at all.
pub fn speech_bounds(analysis: &VadAnalysis, total: usize, pad_ms: u32) -> Option<(usize, usize)> {
    let pad = (pad_ms as usize * 16_000) / 1000;
    let first = analysis
        .closed
        .first()
        .map(|s| s.start)
        .into_iter()
        .chain(analysis.open_start)
        .min()?;
    let last = analysis
        .closed
        .last()
        .map(|s| s.end)
        .into_iter()
        .chain(analysis.open_start.map(|_| total))
        .max()
        .unwrap_or(total);
    Some((first.saturating_sub(pad), (last + pad).min(total)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_bounds_pads_and_clamps() {
        let a = VadAnalysis {
            closed: vec![SampleSegment {
                start: 16_000,
                end: 32_000,
            }],
            open_start: None,
        };
        let (s, e) = speech_bounds(&a, 40_000, 150).unwrap();
        assert_eq!(s, 16_000 - 2_400);
        assert_eq!(e, 32_000 + 2_400);

        // Open speech extends to the end of the buffer.
        let a = VadAnalysis {
            closed: vec![],
            open_start: Some(8_000),
        };
        let (s, e) = speech_bounds(&a, 20_000, 150).unwrap();
        assert_eq!(s, 8_000 - 2_400);
        assert_eq!(e, 20_000);

        // Silence only.
        let a = VadAnalysis {
            closed: vec![],
            open_start: None,
        };
        assert!(speech_bounds(&a, 20_000, 150).is_none());
    }
}
