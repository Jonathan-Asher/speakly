//! Merge speaker turns into whisper segments: each segment gets the speaker
//! with the greatest temporal overlap; ties and no-overlap fall back to the
//! previous segment's speaker (continuity). Segment boundaries are never
//! changed here — grouping consecutive same-speaker segments is presentation.

use speakly_engine_types::Segment;

use super::SpeakerTurn;

fn overlap_ms(a0: u64, a1: u64, b0: u64, b1: u64) -> u64 {
    a1.min(b1).saturating_sub(a0.max(b0))
}

/// Label `segments` in place with "Speaker N" (1-based by cluster index).
pub fn assign_speakers(segments: &mut [Segment], turns: &[SpeakerTurn]) {
    let mut previous: Option<u32> = None;
    for segment in segments.iter_mut() {
        let mut best: Option<(u32, u64)> = None;
        for turn in turns {
            let ov = overlap_ms(segment.start_ms, segment.end_ms, turn.t0_ms, turn.t1_ms);
            if ov == 0 {
                continue;
            }
            best = match best {
                // Strictly-greater keeps the earlier turn on exact ties, and
                // the continuity rule below prefers the previous speaker.
                Some((_, best_ov)) if ov > best_ov => Some((turn.speaker, ov)),
                Some((spk, best_ov)) if ov == best_ov && Some(turn.speaker) == previous => {
                    let _ = spk;
                    Some((turn.speaker, ov))
                }
                Some(kept) => Some(kept),
                None => Some((turn.speaker, ov)),
            };
        }
        let chosen = match best {
            Some((speaker, _)) => Some(speaker),
            // No overlapping turn (silence-adjacent segment): continuity.
            None => previous,
        };
        segment.speaker = chosen.map(|s| format!("Speaker {}", s + 1));
        previous = chosen;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(t0: u64, t1: u64) -> Segment {
        Segment {
            start_ms: t0,
            end_ms: t1,
            speaker: None,
            text: "x".into(),
        }
    }

    fn turn(t0: u64, t1: u64, speaker: u32) -> SpeakerTurn {
        SpeakerTurn {
            t0_ms: t0,
            t1_ms: t1,
            speaker,
        }
    }

    #[test]
    fn assigns_by_max_overlap() {
        let mut segs = vec![seg(0, 1000), seg(1000, 3000)];
        let turns = vec![turn(0, 1200, 0), turn(1200, 3000, 1)];
        assign_speakers(&mut segs, &turns);
        assert_eq!(segs[0].speaker.as_deref(), Some("Speaker 1"));
        // 200 ms of speaker 0 vs 1800 ms of speaker 1.
        assert_eq!(segs[1].speaker.as_deref(), Some("Speaker 2"));
    }

    #[test]
    fn tie_prefers_previous_speaker() {
        let mut segs = vec![seg(0, 1000), seg(1000, 2000)];
        // Second segment overlaps both turns by exactly 500 ms.
        let turns = vec![turn(0, 1500, 0), turn(1500, 2500, 1)];
        assign_speakers(&mut segs, &turns);
        assert_eq!(segs[0].speaker.as_deref(), Some("Speaker 1"));
        assert_eq!(segs[1].speaker.as_deref(), Some("Speaker 1"));
    }

    #[test]
    fn no_overlap_falls_back_to_previous_then_none() {
        let mut segs = vec![seg(0, 500), seg(5000, 6000)];
        let turns = vec![turn(0, 600, 2)];
        assign_speakers(&mut segs, &turns);
        assert_eq!(segs[0].speaker.as_deref(), Some("Speaker 3"));
        // Second segment overlaps nothing → continuity with the first.
        assert_eq!(segs[1].speaker.as_deref(), Some("Speaker 3"));

        let mut lonely = vec![seg(10_000, 11_000)];
        assign_speakers(&mut lonely, &[]);
        assert_eq!(lonely[0].speaker, None);
    }
}
