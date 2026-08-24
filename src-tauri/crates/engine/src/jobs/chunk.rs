//! Split long audio into ≤30 s chunks, cutting at the quietest point in the
//! last 5 s of each window so words aren't sliced mid-syllable. Whisper's
//! integrated VAD replaces this heuristic later.

use std::ops::Range;

use crate::audio::resample::WHISPER_RATE;

const MAX_CHUNK_S: usize = 30;
const SEARCH_BACK_S: usize = 5;
const RMS_WINDOW_MS: usize = 100;

pub fn split_chunks(samples: &[f32]) -> Vec<Range<usize>> {
    let rate = WHISPER_RATE as usize;
    let max = MAX_CHUNK_S * rate;
    let mut chunks = Vec::new();
    let mut pos = 0;

    while samples.len() - pos > max {
        let search_start = pos + (MAX_CHUNK_S - SEARCH_BACK_S) * rate;
        let search_end = pos + max;
        let cut = quietest_point(&samples[search_start..search_end]) + search_start;
        chunks.push(pos..cut);
        pos = cut;
    }
    if pos < samples.len() {
        chunks.push(pos..samples.len());
    }
    chunks
}

/// Index (relative) of the center of the lowest-RMS window in `span`.
fn quietest_point(span: &[f32]) -> usize {
    let win = (WHISPER_RATE as usize) * RMS_WINDOW_MS / 1000;
    if span.len() <= win {
        return span.len() / 2;
    }
    let mut best_start = 0;
    let mut best_energy = f32::MAX;
    let mut start = 0;
    while start + win <= span.len() {
        let energy: f32 = span[start..start + win].iter().map(|s| s * s).sum();
        if energy < best_energy {
            best_energy = energy;
            best_start = start;
        }
        start += win / 2;
    }
    best_start + win / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = WHISPER_RATE as usize;

    fn tone(secs: usize) -> Vec<f32> {
        (0..secs * RATE)
            .map(|i| (i as f32 * 0.05).sin() * 0.5)
            .collect()
    }

    #[test]
    fn short_audio_is_one_chunk() {
        let chunks = split_chunks(&tone(10));
        assert_eq!(chunks, vec![0..10 * RATE]);
    }

    #[test]
    fn chunks_cover_everything_contiguously_and_respect_max() {
        let samples = tone(95);
        let chunks = split_chunks(&samples);
        assert!(chunks.len() >= 4);
        assert_eq!(chunks.first().unwrap().start, 0);
        assert_eq!(chunks.last().unwrap().end, samples.len());
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        for c in &chunks {
            assert!(c.end - c.start <= 30 * RATE);
        }
    }

    #[test]
    fn cut_lands_in_silence() {
        // Loud tone with silence at 27–28 s: the first cut must land inside it.
        let mut samples = tone(60);
        let silence = (27 * RATE)..(28 * RATE);
        for s in &mut samples[silence.clone()] {
            *s = 0.0;
        }
        let chunks = split_chunks(&samples);
        assert!(
            silence.contains(&chunks[0].end),
            "cut at {} not inside 27-28s silence",
            chunks[0].end
        );
    }
}
