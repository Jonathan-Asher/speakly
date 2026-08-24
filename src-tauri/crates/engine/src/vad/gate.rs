//! Pure hysteresis gate over per-frame speech probabilities. Backend-agnostic
//! and side-effect free so the state machine is unit-testable with synthetic
//! sequences; the Silero backend in `vad::mod` produces the probabilities.

#[derive(Debug, Clone, Copy)]
pub struct GateConfig {
    /// Probability at or above which a frame counts toward speech onset.
    pub on_threshold: f32,
    /// Consecutive onset frames required to open a segment.
    pub on_frames: usize,
    /// Probability below which a frame counts as silence while in speech.
    pub off_threshold: f32,
    /// Sustained silence needed to close a segment.
    pub off_ms: u32,
    /// Closed segments shorter than this are discarded (breath, key clicks).
    pub min_speech_ms: u32,
    /// Force a split when an open segment grows past this.
    pub max_segment_ms: u32,
    /// Duration of one probability frame (Silero at 16 kHz: 512 samples = 32 ms).
    pub frame_ms: u32,
}

impl GateConfig {
    pub fn dictation(frame_ms: u32) -> Self {
        Self {
            on_threshold: 0.6,
            on_frames: 2,
            off_threshold: 0.35,
            off_ms: 300,
            min_speech_ms: 250,
            max_segment_ms: 25_000,
            frame_ms: frame_ms.max(1),
        }
    }
}

/// A speech segment in frame indices: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSegment {
    pub start: usize,
    pub end: usize,
}

/// Result of running the gate over a probability sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateOutput {
    /// Segments that ended in sustained silence (safe to commit).
    pub closed: Vec<FrameSegment>,
    /// Start frame of speech still open at the end of the sequence.
    pub open_start: Option<usize>,
}

enum State {
    Silence,
    Candidate { start: usize, run: usize },
    Speech { start: usize, quiet_run: usize },
}

/// Run the full gate over `probs`. Stateless across calls — dictation windows
/// are short (≤ max segment), so re-running per tick is cheap and avoids
/// carry-over bugs.
pub fn run(cfg: &GateConfig, probs: &[f32]) -> GateOutput {
    let off_frames = (cfg.off_ms / cfg.frame_ms).max(1) as usize;
    let min_speech_frames = (cfg.min_speech_ms / cfg.frame_ms).max(1) as usize;
    let max_segment_frames = (cfg.max_segment_ms / cfg.frame_ms).max(1) as usize;

    let mut closed = Vec::new();
    let mut state = State::Silence;

    for (i, &p) in probs.iter().enumerate() {
        state = match state {
            State::Silence => {
                if p >= cfg.on_threshold {
                    if cfg.on_frames <= 1 {
                        State::Speech {
                            start: i,
                            quiet_run: 0,
                        }
                    } else {
                        State::Candidate { start: i, run: 1 }
                    }
                } else {
                    State::Silence
                }
            }
            State::Candidate { start, run } => {
                if p >= cfg.on_threshold {
                    if run + 1 >= cfg.on_frames {
                        State::Speech {
                            start,
                            quiet_run: 0,
                        }
                    } else {
                        State::Candidate {
                            start,
                            run: run + 1,
                        }
                    }
                } else {
                    State::Silence
                }
            }
            State::Speech { start, quiet_run } => {
                if p < cfg.off_threshold {
                    let quiet_run = quiet_run + 1;
                    if quiet_run >= off_frames {
                        // Close at the last speech frame.
                        let end = i + 1 - quiet_run;
                        if end - start >= min_speech_frames {
                            closed.push(FrameSegment { start, end });
                        }
                        State::Silence
                    } else {
                        State::Speech { start, quiet_run }
                    }
                } else if i + 1 - start >= max_segment_frames {
                    // Forced split: close here, stay in speech from this frame.
                    closed.push(FrameSegment { start, end: i + 1 });
                    State::Speech {
                        start: i + 1,
                        quiet_run: 0,
                    }
                } else {
                    State::Speech {
                        start,
                        quiet_run: 0,
                    }
                }
            }
        };
    }

    let open_start = match state {
        State::Speech { start, .. } => Some(start),
        State::Candidate { start, .. } => Some(start),
        State::Silence => None,
    };
    GateOutput { closed, open_start }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> GateConfig {
        GateConfig::dictation(32) // off=9 frames, min_speech=7 frames, max=781
    }

    #[test]
    fn onset_needs_two_frames() {
        // Single spike never opens.
        let mut probs = vec![0.1; 20];
        probs[5] = 0.9;
        let out = run(&cfg(), &probs);
        assert!(out.closed.is_empty());
        assert_eq!(out.open_start, None);

        // Two consecutive frames open (and stay open without silence).
        let probs = [vec![0.1; 3], vec![0.9; 12]].concat();
        let out = run(&cfg(), &probs);
        assert_eq!(out.open_start, Some(3));
    }

    #[test]
    fn short_dip_does_not_close() {
        // Speech, a 5-frame dip (160 ms < 300 ms), speech again.
        let probs = [vec![0.9; 10], vec![0.1; 5], vec![0.9; 10]].concat();
        let out = run(&cfg(), &probs);
        assert!(out.closed.is_empty());
        assert_eq!(out.open_start, Some(0));
    }

    #[test]
    fn sustained_silence_closes_at_last_speech_frame() {
        let probs = [vec![0.9; 10], vec![0.1; 12]].concat();
        let out = run(&cfg(), &probs);
        assert_eq!(
            out.closed,
            vec![FrameSegment { start: 0, end: 10 }],
            "closes exactly where speech ended"
        );
        assert_eq!(out.open_start, None);
    }

    #[test]
    fn too_short_speech_is_discarded() {
        // 4 frames of speech (128 ms < 250 ms min) then long silence.
        let probs = [vec![0.9; 4], vec![0.1; 12]].concat();
        let out = run(&cfg(), &probs);
        assert!(out.closed.is_empty());
        assert_eq!(out.open_start, None);
    }

    #[test]
    fn mid_band_probability_keeps_speech_open() {
        // 0.5 is below on (0.6) but above off (0.35): sustains, never closes.
        let probs = [vec![0.9; 8], vec![0.5; 20]].concat();
        let out = run(&cfg(), &probs);
        assert!(out.closed.is_empty());
        assert_eq!(out.open_start, Some(0));
    }

    #[test]
    fn max_segment_forces_split_and_reopens() {
        let mut c = cfg();
        c.max_segment_ms = 320; // 10 frames
        let probs = vec![0.9; 25];
        let out = run(&c, &probs);
        assert_eq!(
            out.closed,
            vec![
                FrameSegment { start: 0, end: 10 },
                FrameSegment { start: 10, end: 20 }
            ]
        );
        assert_eq!(out.open_start, Some(20));
    }

    #[test]
    fn two_utterances_produce_two_segments() {
        let probs = [vec![0.9; 10], vec![0.1; 12], vec![0.9; 10], vec![0.1; 12]].concat();
        let out = run(&cfg(), &probs);
        assert_eq!(out.closed.len(), 2);
        assert_eq!(out.closed[1], FrameSegment { start: 22, end: 32 });
    }
}
