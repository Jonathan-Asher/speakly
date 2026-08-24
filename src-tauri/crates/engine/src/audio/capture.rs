//! Microphone capture. `cpal::Stream` is not `Send`, so a dedicated thread
//! owns the stream and is driven over a command channel. Samples are
//! downmixed to mono f32 at the device's native rate; resampling to 16 kHz
//! happens at utterance end (see [`crate::audio::resample`]).

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct CaptureService {
    cmd_tx: Sender<Cmd>,
}

enum Cmd {
    Start {
        out: Sender<Vec<f32>>,
        reply: Sender<Result<u32, String>>,
    },
    Stop,
}

impl CaptureService {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        std::thread::Builder::new()
            .name("speakly-capture".into())
            .spawn(move || capture_thread(cmd_rx))
            .expect("spawn capture thread");
        Self { cmd_tx }
    }

    /// Open the default input device and start streaming mono f32 chunks into
    /// `out`. Returns the device's native sample rate. The `out` sender is
    /// dropped when capture stops, closing the channel.
    pub fn start(&self, out: Sender<Vec<f32>>) -> Result<u32, String> {
        let (reply_tx, reply_rx) = bounded(1);
        self.cmd_tx
            .send(Cmd::Start {
                out,
                reply: reply_tx,
            })
            .map_err(|_| "capture thread gone".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "capture thread gone".to_string())?
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(Cmd::Stop);
    }
}

fn capture_thread(cmd_rx: Receiver<Cmd>) {
    // The live stream (and the `out` sender captured by its callback) lives
    // here; dropping it stops CoreAudio callbacks and closes the channel.
    let mut active: Option<cpal::Stream> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Start { out, reply } => match open_stream(out) {
                Ok((stream, rate)) => {
                    // Drop any previous stream, hold the new one alive.
                    drop(active.replace(stream));
                    let _ = reply.send(Ok(rate));
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            },
            Cmd::Stop => drop(active.take()),
        }
    }
}

fn open_stream(out: Sender<Vec<f32>>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("no input device available")?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("input config: {e}"))?;
    let sample_rate: u32 = config.sample_rate();
    let channels = config.channels() as usize;

    let err_out = out.clone();
    let stream = device
        .build_input_stream(
            config.into(),
            move |data: &[f32], _| {
                let mono: Vec<f32> = if channels == 1 {
                    data.to_vec()
                } else {
                    data.chunks(channels)
                        .map(|f| f.iter().sum::<f32>() / channels as f32)
                        .collect()
                };
                let _ = out.send(mono);
            },
            move |e| {
                tracing::warn!("capture stream error: {e}");
                // Dropping nothing here; the collector notices when the
                // channel closes on stop.
                let _ = &err_out;
            },
            None,
        )
        .map_err(|e| format!("build input stream: {e}"))?;
    stream.play().map_err(|e| format!("start stream: {e}"))?;
    Ok((stream, sample_rate))
}
