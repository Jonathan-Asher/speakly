//! Framed-message codec shared with the Swift sidecar (see
//! `sidecars/speakly-syscap`): `[u32 le type][u32 le len][payload]`.
//! AUDIO payload = `u64 le pts_ns` + f32le mono samples.

use std::io::{self, Read};

pub const TYPE_AUDIO: u32 = 1;
pub const TYPE_STATUS: u32 = 2;
pub const TYPE_ERROR: u32 = 3;

/// Backstop against a corrupt length header; audio frames are ~KB-scale.
const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Audio { pts_ns: u64, samples: Vec<f32> },
    Status(String),
    Error(String),
}

/// Read one frame. `Ok(None)` on clean EOF at a frame boundary; unknown frame
/// types are skipped (forward compatibility).
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Frame>> {
    loop {
        let mut head = [0u8; 8];
        if !read_exact_or_eof(r, &mut head)? {
            return Ok(None);
        }
        let ty = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if len > MAX_FRAME_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("frame length {len} exceeds cap"),
            ));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        match ty {
            TYPE_AUDIO => {
                if len < 8 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "audio frame shorter than pts header",
                    ));
                }
                let pts_ns = u64::from_le_bytes(payload[..8].try_into().unwrap());
                let samples = payload[8..]
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                    .collect();
                return Ok(Some(Frame::Audio { pts_ns, samples }));
            }
            TYPE_STATUS => {
                return Ok(Some(Frame::Status(
                    String::from_utf8_lossy(&payload).into_owned(),
                )))
            }
            TYPE_ERROR => {
                return Ok(Some(Frame::Error(
                    String::from_utf8_lossy(&payload).into_owned(),
                )))
            }
            _ => continue,
        }
    }
}

/// True when the buffer was filled; false on EOF before the first byte.
fn read_exact_or_eof(r: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..])? {
            0 if filled == 0 => return Ok(false),
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "eof mid-frame",
                ))
            }
            n => filled += n,
        }
    }
    Ok(true)
}

#[cfg(test)]
pub fn encode_audio(pts_ns: u64, samples: &[f32]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8 + samples.len() * 4);
    payload.extend_from_slice(&pts_ns.to_le_bytes());
    for s in samples {
        payload.extend_from_slice(&s.to_le_bytes());
    }
    encode(TYPE_AUDIO, &payload)
}

#[cfg(test)]
pub fn encode_text(ty: u32, text: &str) -> Vec<u8> {
    encode(ty, text.as_bytes())
}

#[cfg(test)]
fn encode(ty: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&ty.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_all_frame_kinds_then_eof() {
        let mut bytes = encode_audio(123_456, &[0.5, -0.25, 1.0]);
        bytes.extend(encode_text(TYPE_STATUS, r#"{"event":"started"}"#));
        bytes.extend(encode_text(TYPE_ERROR, r#"{"error":"x"}"#));
        // Unknown type is skipped.
        bytes.extend(encode(99, b"ignore me"));
        bytes.extend(encode_text(TYPE_STATUS, r#"{"event":"stopped"}"#));

        let mut cur = Cursor::new(bytes);
        assert_eq!(
            read_frame(&mut cur).unwrap(),
            Some(Frame::Audio {
                pts_ns: 123_456,
                samples: vec![0.5, -0.25, 1.0]
            })
        );
        assert!(
            matches!(read_frame(&mut cur).unwrap(), Some(Frame::Status(s)) if s.contains("started"))
        );
        assert!(matches!(read_frame(&mut cur).unwrap(), Some(Frame::Error(e)) if e.contains("x")));
        assert!(
            matches!(read_frame(&mut cur).unwrap(), Some(Frame::Status(s)) if s.contains("stopped"))
        );
        assert_eq!(read_frame(&mut cur).unwrap(), None);
    }

    #[test]
    fn eof_mid_frame_is_an_error() {
        let bytes = encode_audio(1, &[0.1, 0.2]);
        let mut cur = Cursor::new(&bytes[..bytes.len() - 3]);
        assert!(read_frame(&mut cur).is_err());
    }
}
