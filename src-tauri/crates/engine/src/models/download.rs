//! Streamed, resumable model downloads: `.part` file + Range resume, disk
//! precheck, throttled progress, cancellation that keeps the partial file,
//! atomic rename on completion. Blocking (ureq) — each download runs on its
//! own thread owned by the ModelService.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::registry::{file_name, ModelInfo};

const PROGRESS_EVERY: Duration = Duration::from_millis(250);
const BUF_SIZE: usize = 128 * 1024;

pub fn dest_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(file_name(id))
}

fn part_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{}.part", file_name(id)))
}

/// Parse the total size out of a `Content-Range: bytes 100-999/12345` header.
pub fn total_from_content_range(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.trim().parse().ok()
}

/// Download `info` into `dir`. Progress callback receives
/// `(bytes_done, total, bytes_per_sec)` at most every 250 ms plus once at the
/// end. Cancellation keeps the `.part` file for resume. Returns the final path.
pub fn download(
    info: &ModelInfo,
    dir: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>, u64),
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create models dir: {e}"))?;
    let dest = dest_path(dir, info.id);
    if dest.is_file() {
        return Ok(dest);
    }
    let part = part_path(dir, info.id);
    let resume_from = part.metadata().map(|m| m.len()).unwrap_or(0);

    let needed = info.size_bytes.saturating_sub(resume_from);
    let available = fs2::available_space(dir).map_err(|e| format!("disk space: {e}"))?;
    if available < needed + needed / 10 {
        return Err(format!(
            "not enough disk space: need ~{} MB free",
            (needed + needed / 10) / 1_000_000
        ));
    }

    let mut request = ureq::get(info.url);
    if resume_from > 0 {
        request = request.set("Range", &format!("bytes={resume_from}-"));
    }
    let response = request.call().map_err(|e| format!("request: {e}"))?;

    let (mut written, total, mut file) = match response.status() {
        206 => {
            let total = response
                .header("Content-Range")
                .and_then(total_from_content_range)
                .or_else(|| {
                    response
                        .header("Content-Length")
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(|len| len + resume_from)
                });
            let file = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&part)
                .map_err(|e| format!("open part: {e}"))?;
            (resume_from, total, file)
        }
        200 => {
            // Server ignored the range request — start over.
            let total = response
                .header("Content-Length")
                .and_then(|v| v.parse::<u64>().ok());
            let file = std::fs::File::create(&part).map_err(|e| format!("create part: {e}"))?;
            (0u64, total, file)
        }
        s => return Err(format!("unexpected HTTP status {s}")),
    };

    let mut reader = response.into_reader();
    let mut buf = vec![0u8; BUF_SIZE];
    let mut last_emit = Instant::now();
    let mut window_bytes: u64 = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write: {e}"))?;
        written += n as u64;
        window_bytes += n as u64;

        let elapsed = last_emit.elapsed();
        if elapsed >= PROGRESS_EVERY {
            let bps = (window_bytes as f64 / elapsed.as_secs_f64()) as u64;
            progress(written, total, bps);
            last_emit = Instant::now();
            window_bytes = 0;
        }
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    drop(file);

    if let Some(total) = total {
        if written != total {
            return Err(format!(
                "incomplete download: got {written} of {total} bytes (will resume on retry)"
            ));
        }
    }

    std::fs::rename(&part, &dest).map_err(|e| format!("finalize: {e}"))?;
    progress(written, total, 0);
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_range_total() {
        assert_eq!(total_from_content_range("bytes 100-999/12345"), Some(12345));
        assert_eq!(total_from_content_range("bytes 0-0/1"), Some(1));
        assert_eq!(total_from_content_range("garbage"), None);
    }

    #[test]
    fn part_and_dest_naming() {
        let dir = Path::new("/tmp/models");
        assert_eq!(
            dest_path(dir, "he-turbo"),
            Path::new("/tmp/models/ggml-he-turbo.bin")
        );
        assert_eq!(
            part_path(dir, "he-turbo"),
            Path::new("/tmp/models/ggml-he-turbo.bin.part")
        );
    }
}
