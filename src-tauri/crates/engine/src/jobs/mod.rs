//! Sequential file-transcription queue: decode → chunk → low-priority decode
//! per chunk → segments streamed out as they land. Dictation always preempts
//! at chunk granularity via the STT lane's priority queues.

pub mod chunk;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::audio::resample::WHISPER_RATE;
use crate::diarize::DiarizeOpts;
use crate::audio::{decode, ffmpeg_fallback};
use crate::stt::{scaled_audio_ctx, DecodeRequest, SttService};
use crate::{EngineEvent, EventSink};

#[derive(Clone)]
pub struct FileJobSpec {
    pub id: String,
    pub path: String,
    /// Whisper language code, or "auto".
    pub language: String,
    pub model_id: String,
    pub model_path: String,
    pub scale_audio_ctx: bool,
    pub diarize: Option<DiarizeOpts>,
}

pub struct QueueOptions {
    pub path: String,
    pub language: String,
    pub model_id: String,
    pub model_path: String,
    pub scale_audio_ctx: bool,
    pub diarize: Option<DiarizeOpts>,
}

pub struct FileJobService {
    queue_tx: Sender<FileJobSpec>,
    cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    counter: AtomicU64,
}

impl FileJobService {
    pub fn new(stt: SttService, sink: Arc<dyn EventSink>) -> Self {
        let (queue_tx, queue_rx) = unbounded::<FileJobSpec>();
        let cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>> = Arc::default();
        let thread_cancels = Arc::clone(&cancels);
        std::thread::Builder::new()
            .name("speakly-jobs".into())
            .spawn(move || job_thread(queue_rx, stt, sink, thread_cancels))
            .expect("spawn job thread");
        Self {
            queue_tx,
            cancels,
            counter: AtomicU64::new(1),
        }
    }

    pub fn enqueue(&self, opts: QueueOptions) -> String {
        let id = format!("job-{}", self.counter.fetch_add(1, Ordering::Relaxed));
        self.cancels
            .lock()
            .unwrap()
            .insert(id.clone(), Arc::new(AtomicBool::new(false)));
        let _ = self.queue_tx.send(FileJobSpec {
            id: id.clone(),
            path: opts.path,
            language: opts.language,
            model_id: opts.model_id,
            model_path: opts.model_path,
            scale_audio_ctx: opts.scale_audio_ctx,
            diarize: opts.diarize,
        });
        id
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.cancels.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

fn job_thread(
    queue_rx: Receiver<FileJobSpec>,
    stt: SttService,
    sink: Arc<dyn EventSink>,
    cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
) {
    while let Ok(spec) = queue_rx.recv() {
        let flag = cancels
            .lock()
            .unwrap()
            .get(&spec.id)
            .cloned()
            .unwrap_or_default();
        run_job(&spec, &stt, &sink, &flag);
        cancels.lock().unwrap().remove(&spec.id);
    }
}

fn run_job(spec: &FileJobSpec, stt: &SttService, sink: &Arc<dyn EventSink>, cancel: &AtomicBool) {
    let id = spec.id.clone();
    let cancelled = || cancel.load(Ordering::Relaxed);
    let emit_cancel = || sink.emit(EngineEvent::JobCancelled { id: id.clone() });

    if cancelled() {
        emit_cancel();
        return;
    }
    sink.emit(EngineEvent::JobProgress {
        id: id.clone(),
        stage: "decoding".into(),
        pct: 0.0,
    });

    let path = Path::new(&spec.path);
    let audio = match decode::decode_file_16k(path) {
        Ok(a) => a,
        Err(decode::DecodeError::Unsupported(sym_err)) => {
            match ffmpeg_fallback::find_ffmpeg()
                .ok_or(&sym_err)
                .and_then(|ffmpeg| {
                    ffmpeg_fallback::decode_file_16k(&ffmpeg, path).map_err(|e| {
                        tracing::warn!("ffmpeg fallback failed: {e}");
                        &sym_err
                    })
                }) {
                Ok(a) => a,
                Err(e) => {
                    sink.emit(EngineEvent::JobError {
                        id,
                        message: e.to_string(),
                    });
                    return;
                }
            }
        }
        Err(e) => {
            sink.emit(EngineEvent::JobError {
                id,
                message: e.to_string(),
            });
            return;
        }
    };
    if cancelled() {
        emit_cancel();
        return;
    }

    let total = audio.len();
    let duration_ms = (total as u64 * 1000) / WHISPER_RATE as u64;
    let chunks = chunk::split_chunks(&audio);
    // Retained for diarization relabeling after transcription completes.
    let mut collected: Vec<speakly_engine_types::Segment> = Vec::new();
    for range in chunks {
        if cancelled() {
            emit_cancel();
            return;
        }
        let offset_ms = (range.start as u64 * 1000) / WHISPER_RATE as u64;
        let chunk_audio = audio[range.clone()].to_vec();
        let audio_ctx = spec
            .scale_audio_ctx
            .then(|| scaled_audio_ctx(chunk_audio.len()));
        let result = stt.decode_low(DecodeRequest {
            model_id: spec.model_id.clone(),
            model_path: spec.model_path.clone(),
            language: spec.language.clone(),
            audio: chunk_audio,
            audio_ctx,
            with_timestamps: true,
        });
        match result {
            Ok(outcome) => {
                for mut segment in outcome.segments {
                    segment.start_ms += offset_ms;
                    segment.end_ms += offset_ms;
                    collected.push(segment.clone());
                    sink.emit(EngineEvent::JobSegment {
                        id: id.clone(),
                        segment,
                    });
                }
                sink.emit(EngineEvent::JobProgress {
                    id: id.clone(),
                    stage: "transcribing".into(),
                    pct: (range.end as f32 / total as f32).min(1.0),
                });
            }
            Err(message) => {
                sink.emit(EngineEvent::JobError { id, message });
                return;
            }
        }
    }
    // Optional speaker identification over the full decoded audio (the 16 kHz
    // buffer is retained in memory — ~115 MB per audio hour, fine for files).
    if let Some(diar) = &spec.diarize {
        if !collected.is_empty() {
            if cancelled() {
                emit_cancel();
                return;
            }
            sink.emit(EngineEvent::JobProgress {
                id: id.clone(),
                stage: "diarizing".into(),
                pct: 0.0,
            });
            match crate::diarize::diarize(
                &audio,
                Path::new(&diar.seg_model_path),
                Path::new(&diar.emb_model_path),
                diar.num_speakers,
            ) {
                Ok(turns) => {
                    crate::diarize::merge::assign_speakers(&mut collected, &turns);
                    sink.emit(EngineEvent::JobSegmentsRelabeled {
                        id: id.clone(),
                        segments: collected,
                    });
                }
                Err(message) => sink.emit(EngineEvent::Warning {
                    code: "diarize".into(),
                    message,
                }),
            }
            sink.emit(EngineEvent::JobProgress {
                id: id.clone(),
                stage: "diarizing".into(),
                pct: 1.0,
            });
        }
    }
    sink.emit(EngineEvent::JobDone { id, duration_ms });
}
