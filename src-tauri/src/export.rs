//! Transcript rendering (txt/md/srt/vtt) and save-dialog export. Stateless:
//! the frontend sends the segments it is showing.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ExportSegment {
    #[serde(rename = "startMs")]
    pub start_ms: u64,
    #[serde(rename = "endMs")]
    pub end_ms: u64,
    #[serde(default)]
    pub speaker: Option<String>,
    pub text: String,
}

/// "Speaker N: text" when a speaker label exists, else just the text.
fn spoken_line(s: &ExportSegment) -> String {
    match s.speaker.as_deref() {
        Some(speaker) if !speaker.is_empty() => format!("{speaker}: {}", s.text.trim()),
        _ => s.text.trim().to_string(),
    }
}

pub fn render(format: &str, segments: &[ExportSegment]) -> Result<String, String> {
    match format {
        "txt" => Ok(segments
            .iter()
            .map(spoken_line)
            .collect::<Vec<_>>()
            .join("\n")),
        "md" => Ok(segments
            .iter()
            .map(|s| format!("**[{}]** {}", fmt_mmss(s.start_ms), spoken_line(s)))
            .collect::<Vec<_>>()
            .join("\n\n")),
        "srt" => Ok(segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{}\n{} --> {}\n{}\n",
                    i + 1,
                    fmt_srt(s.start_ms),
                    fmt_srt(s.end_ms),
                    spoken_line(s)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")),
        "vtt" => {
            let body = segments
                .iter()
                .map(|s| {
                    let cue = match s.speaker.as_deref() {
                        Some(speaker) if !speaker.is_empty() => {
                            format!("<v {speaker}>{}</v>", s.text.trim())
                        }
                        _ => s.text.trim().to_string(),
                    };
                    format!(
                        "{} --> {}\n{}\n",
                        fmt_vtt(s.start_ms),
                        fmt_vtt(s.end_ms),
                        cue
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!("WEBVTT\n\n{body}"))
        }
        other => Err(format!("unknown export format: {other}")),
    }
}

pub fn extension(format: &str) -> &'static str {
    match format {
        "md" => "md",
        "srt" => "srt",
        "vtt" => "vtt",
        _ => "txt",
    }
}

fn split_ms(ms: u64) -> (u64, u64, u64, u64) {
    (
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000,
    )
}

fn fmt_srt(ms: u64) -> String {
    let (h, m, s, milli) = split_ms(ms);
    format!("{h:02}:{m:02}:{s:02},{milli:03}")
}

fn fmt_vtt(ms: u64) -> String {
    let (h, m, s, milli) = split_ms(ms);
    format!("{h:02}:{m:02}:{s:02}.{milli:03}")
}

pub fn fmt_mmss(ms: u64) -> String {
    let total_s = ms / 1000;
    format!("{:02}:{:02}", total_s / 60, total_s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs() -> Vec<ExportSegment> {
        vec![
            ExportSegment {
                start_ms: 0,
                end_ms: 1500,
                speaker: None,
                text: " שלום עולם ".into(),
            },
            ExportSegment {
                start_ms: 61_250,
                end_ms: 3_726_040,
                speaker: None,
                text: "hello".into(),
            },
        ]
    }

    fn labeled() -> Vec<ExportSegment> {
        vec![
            ExportSegment {
                start_ms: 0,
                end_ms: 1000,
                speaker: Some("Speaker 1".into()),
                text: "hi".into(),
            },
            ExportSegment {
                start_ms: 1000,
                end_ms: 2000,
                speaker: Some("Dana".into()),
                text: "שלום".into(),
            },
        ]
    }

    #[test]
    fn speaker_labels_render_per_format() {
        assert_eq!(
            render("txt", &labeled()).unwrap(),
            "Speaker 1: hi\nDana: שלום"
        );
        assert!(render("srt", &labeled()).unwrap().contains("Dana: שלום"));
        let vtt = render("vtt", &labeled()).unwrap();
        assert!(vtt.contains("<v Speaker 1>hi</v>"));
        assert!(vtt.contains("<v Dana>שלום</v>"));
        assert!(render("md", &labeled())
            .unwrap()
            .contains("**[00:00]** Speaker 1: hi"));
    }

    #[test]
    fn srt_timestamps_and_indices() {
        let out = render("srt", &segs()).unwrap();
        assert!(out.starts_with("1\n00:00:00,000 --> 00:00:01,500\nשלום עולם\n"));
        assert!(out.contains("2\n00:01:01,250 --> 01:02:06,040\nhello\n"));
    }

    #[test]
    fn vtt_header_and_dot_millis() {
        let out = render("vtt", &segs()).unwrap();
        assert!(out.starts_with("WEBVTT\n\n"));
        assert!(out.contains("00:01:01.250 --> 01:02:06.040"));
    }

    #[test]
    fn txt_and_md() {
        assert_eq!(render("txt", &segs()).unwrap(), "שלום עולם\nhello");
        assert!(render("md", &segs()).unwrap().contains("**[01:01]** hello"));
        assert!(render("docx", &segs()).is_err());
    }
}
