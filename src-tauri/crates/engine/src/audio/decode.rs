//! File decoding via symphonia: probe → decode → mono downmix → 16 kHz.
//! Pure Rust, no FFmpeg required; formats symphonia can't probe fall back to
//! [`super::ffmpeg_fallback`] at the job layer.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::resample::{resample_to_16k, WHISPER_RATE};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "m4a", "mp4", "mov", "aac", "flac", "ogg", "oga", "alac", "caf", "webm", "mka",
    "mkv", "m4b", "m4v",
];

#[derive(Debug)]
pub enum DecodeError {
    /// Container/codec not handled by symphonia — try the ffmpeg fallback.
    Unsupported(String),
    Failed(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Unsupported(m) => write!(f, "unsupported format: {m}"),
            DecodeError::Failed(m) => write!(f, "{m}"),
        }
    }
}

/// Decode any supported media file to 16 kHz mono f32. Resampling runs in
/// ~60 s windows to keep peak memory flat; the per-window resampler reset is
/// inaudible to STT.
pub fn decode_file_16k(path: &Path) -> Result<Vec<f32>, DecodeError> {
    let file = File::open(path).map_err(|e| DecodeError::Failed(format!("open: {e}")))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| {
            DecodeError::Unsupported(format!(
                "{e}; handled formats: {}",
                SUPPORTED_EXTENSIONS.join(", ")
            ))
        })?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| DecodeError::Unsupported("no decodable audio track".into()))?;
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| DecodeError::Failed("track has no sample rate".into()))?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| DecodeError::Unsupported(format!("codec: {e}")))?;

    let window_native = sample_rate as usize * 60;
    let mut native_mono: Vec<f32> = Vec::with_capacity(window_native + 4096);
    let mut out16k: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut buf_spec: Option<SignalSpec> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(DecodeError::Failed(format!("read: {e}"))),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // Skip corrupt packets rather than failing the whole file.
            Err(SymError::DecodeError(_)) => continue,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(DecodeError::Failed(format!("decode: {e}"))),
        };

        let spec = *decoded.spec();
        let needs_new = match (&sample_buf, buf_spec) {
            (Some(buf), Some(prev)) => {
                prev != spec || buf.capacity() < decoded.capacity() * spec.channels.count()
            }
            _ => true,
        };
        if needs_new {
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
            buf_spec = Some(spec);
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);

        let channels = spec.channels.count().max(1);
        if channels == 1 {
            native_mono.extend_from_slice(buf.samples());
        } else {
            native_mono.extend(
                buf.samples()
                    .chunks(channels)
                    .map(|f| f.iter().sum::<f32>() / channels as f32),
            );
        }

        if native_mono.len() >= window_native {
            out16k.extend(resample_to_16k(&native_mono, sample_rate));
            native_mono.clear();
        }
    }
    if !native_mono.is_empty() {
        out16k.extend(resample_to_16k(&native_mono, sample_rate));
    }

    if out16k.is_empty() {
        return Err(DecodeError::Failed("no audio decoded".into()));
    }
    tracing::info!(
        "decoded {:?}: {:.1}s at {sample_rate} Hz",
        path.file_name().unwrap_or_default(),
        out16k.len() as f32 / WHISPER_RATE as f32
    );
    Ok(out16k)
}
