//! Optional FFmpeg pipe-decode for formats symphonia can't probe (e.g.
//! webm/opus). Used only when present on the machine — never a requirement.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

const CANDIDATES: &[&str] = &["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "ffmpeg"];

pub fn find_ffmpeg() -> Option<String> {
    for candidate in CANDIDATES {
        let found = if candidate.contains('/') {
            Path::new(candidate).is_file()
        } else {
            // Bare name: let the OS resolve it via PATH.
            Command::new(candidate)
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        if found {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Decode to 16 kHz mono f32 by piping raw samples out of ffmpeg.
pub fn decode_file_16k(ffmpeg: &str, path: &Path) -> Result<Vec<f32>, String> {
    let mut child = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-f", "f32le", "-acodec", "pcm_f32le", "-ar", "16000", "-ac", "1", "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn ffmpeg: {e}"))?;

    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .expect("piped stdout")
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read ffmpeg output: {e}"))?;

    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }
    let status = child.wait().map_err(|e| format!("ffmpeg: {e}"))?;
    if !status.success() {
        return Err(format!("ffmpeg failed: {}", err.trim()));
    }

    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if samples.is_empty() {
        return Err("ffmpeg produced no audio".into());
    }
    Ok(samples)
}
