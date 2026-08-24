//! Whole-utterance resampling to whisper's 16 kHz mono. Dictation buffers are
//! short (≤ ~30 s), so batch resampling with the high-quality sinc resampler
//! is simpler and better than a streaming one; live streaming partials will
//! add a `FastFixedIn` path later.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

pub const WHISPER_RATE: u32 = 16_000;

pub fn resample_to_16k(samples: &[f32], from_rate: u32) -> Vec<f32> {
    if from_rate == WHISPER_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = WHISPER_RATE as f64 / from_rate as f64;
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    const CHUNK: usize = 4096;
    let mut resampler =
        SincFixedIn::<f32>::new(ratio, 1.0, params, CHUNK, 1).expect("create resampler");

    // The sinc filter delays output and partial chunks are zero-padded to full
    // size, so collect everything (plus one flush), then cut the delay off the
    // front and truncate to the mathematically expected length.
    let delay = resampler.output_delay();
    let expected = (samples.len() as f64 * ratio).round() as usize;

    let mut out = Vec::with_capacity(expected + 2 * CHUNK);
    let mut pos = 0;
    while pos + CHUNK <= samples.len() {
        let frames = resampler
            .process(&[&samples[pos..pos + CHUNK]], None)
            .expect("resample chunk");
        out.extend_from_slice(&frames[0]);
        pos += CHUNK;
    }
    if pos < samples.len() {
        let frames = resampler
            .process_partial(Some(&[&samples[pos..]]), None)
            .expect("resample tail");
        out.extend_from_slice(&frames[0]);
    }
    while out.len() < delay + expected {
        let frames = resampler
            .process_partial::<&[f32]>(None, None)
            .expect("resample flush");
        if frames[0].is_empty() {
            break;
        }
        out.extend_from_slice(&frames[0]);
    }

    let start = delay.min(out.len());
    let end = (start + expected).min(out.len());
    out[start..end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_duration_within_one_percent() {
        let from_rate = 48_000u32;
        let secs = 2.0f32;
        let n = (from_rate as f32 * secs) as usize;
        let input: Vec<f32> = (0..n)
            .map(|i| (i as f32 / from_rate as f32 * 440.0 * std::f32::consts::TAU).sin())
            .collect();
        let out = resample_to_16k(&input, from_rate);
        let expected = (WHISPER_RATE as f32 * secs) as usize;
        let diff = (out.len() as i64 - expected as i64).unsigned_abs() as usize;
        assert!(
            diff < expected / 100,
            "len {} vs expected {}",
            out.len(),
            expected
        );
    }

    #[test]
    fn passthrough_at_16k() {
        let input = vec![0.5f32; 1600];
        assert_eq!(resample_to_16k(&input, WHISPER_RATE).len(), 1600);
    }
}
