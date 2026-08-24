//! Streaming-dictation session state: committed/volatile bookkeeping and the
//! adaptive partial-decode pacing. The ticker thread in `dictation.rs` drives
//! this; everything here is pure enough to unit-test.

/// Result of measuring a partial decode against the latency budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickDecision {
    /// Fast enough — keep the default cadence.
    Keep,
    /// Slow — stretch the tick so partials never pile up.
    Stretch,
    /// Too slow for live partials on this model/machine; final-only mode.
    Disable,
}

pub const DEFAULT_TICK_MS: u64 = 1_200;
pub const STRETCHED_TICK_MS: u64 = 2_500;

/// Judge the first observed partial decode (docs/SPIKES.md budgets).
pub fn judge_first_partial(decode_ms: u64) -> TickDecision {
    match decode_ms {
        0..=899 => TickDecision::Keep,
        900..=2_000 => TickDecision::Stretch,
        _ => TickDecision::Disable,
    }
}

/// Committed text grows only at VAD-closed boundaries and never changes after;
/// the offset is in 16 kHz samples from the utterance start.
#[derive(Debug, Default)]
pub struct SessionState {
    committed_text: String,
    committed_offset: usize,
    pub partials_disabled: bool,
    pub tick_ms: u64,
    pub measured_first: bool,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            tick_ms: DEFAULT_TICK_MS,
            ..Default::default()
        }
    }

    pub fn committed_text(&self) -> &str {
        &self.committed_text
    }

    pub fn committed_offset(&self) -> usize {
        self.committed_offset
    }

    /// Append decoded text for `[committed_offset, new_offset)`.
    pub fn commit(&mut self, text: &str, new_offset: usize) {
        debug_assert!(new_offset >= self.committed_offset);
        let text = text.trim();
        if !text.is_empty() {
            if !self.committed_text.is_empty() {
                self.committed_text.push(' ');
            }
            self.committed_text.push_str(text);
        }
        self.committed_offset = new_offset;
    }

    /// Full transcript given the decoded uncommitted tail.
    pub fn full_text(&self, tail: &str) -> String {
        let tail = tail.trim();
        if self.committed_text.is_empty() {
            tail.to_string()
        } else if tail.is_empty() {
            self.committed_text.clone()
        } else {
            format!("{} {}", self.committed_text, tail)
        }
    }

    /// Apply the first-decode measurement once; returns the decision made.
    pub fn apply_first_measurement(&mut self, decode_ms: u64) -> Option<TickDecision> {
        if self.measured_first {
            return None;
        }
        self.measured_first = true;
        let decision = judge_first_partial(decode_ms);
        match decision {
            TickDecision::Keep => {}
            TickDecision::Stretch => self.tick_ms = STRETCHED_TICK_MS,
            TickDecision::Disable => self.partials_disabled = true,
        }
        Some(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_appends_and_advances() {
        let mut s = SessionState::new();
        s.commit("שלום עולם", 40_000);
        assert_eq!(s.committed_text(), "שלום עולם");
        assert_eq!(s.committed_offset(), 40_000);
        s.commit("  ", 48_000); // whitespace-only commits still advance
        assert_eq!(s.committed_text(), "שלום עולם");
        assert_eq!(s.committed_offset(), 48_000);
        s.commit("מה נשמע", 80_000);
        assert_eq!(s.committed_text(), "שלום עולם מה נשמע");
        assert_eq!(s.full_text("טוב"), "שלום עולם מה נשמע טוב");
        assert_eq!(s.full_text(""), "שלום עולם מה נשמע");
    }

    #[test]
    fn degrade_thresholds() {
        assert_eq!(judge_first_partial(250), TickDecision::Keep);
        assert_eq!(judge_first_partial(899), TickDecision::Keep);
        assert_eq!(judge_first_partial(900), TickDecision::Stretch);
        assert_eq!(judge_first_partial(2_000), TickDecision::Stretch);
        assert_eq!(judge_first_partial(2_001), TickDecision::Disable);
    }

    #[test]
    fn first_measurement_applies_once() {
        let mut s = SessionState::new();
        assert_eq!(
            s.apply_first_measurement(1_500),
            Some(TickDecision::Stretch)
        );
        assert_eq!(s.tick_ms, STRETCHED_TICK_MS);
        assert_eq!(s.apply_first_measurement(3_000), None, "second is ignored");
        assert!(!s.partials_disabled);
    }
}
