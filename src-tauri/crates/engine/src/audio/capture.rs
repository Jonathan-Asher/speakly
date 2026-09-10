//! Microphone capture. `cpal::Stream` is not `Send`, so a dedicated thread
//! owns the stream and is driven over a command channel. Samples are
//! downmixed to mono f32 at the device's native rate; resampling to 16 kHz
//! happens at utterance end (see [`crate::audio::resample`]).
//!
//! Which microphone gets used comes from an ordered preference list, not a
//! single choice: the first connected entry wins, and an empty list means
//! whatever the OS currently calls the default. If the chosen device goes away
//! mid-recording — the normal life of a Bluetooth headset — the thread reopens
//! on the next entry rather than leaving the rest of the utterance silent.

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::ErrorKind;

pub struct CaptureService {
    cmd_tx: Sender<Cmd>,
}

enum Cmd {
    Start {
        out: Sender<Vec<f32>>,
        /// Microphones to try, best first. Entries that are not connected are
        /// skipped; an empty list (or none connected) uses the OS default.
        priority: Vec<String>,
        reply: Sender<Result<u32, String>>,
    },
    /// A live stream reported that its device vanished. `generation` says
    /// which stream complained, so a late error from one already replaced
    /// cannot trigger a second switch.
    DeviceLost { generation: u64 },
    Stop,
}

/// One selectable microphone. The id is cpal's stable device id — it survives
/// reboots and reconnections, unlike the display name, so that is what gets
/// stored in settings.
pub struct InputDevice {
    pub id: String,
    pub name: String,
}

pub fn input_devices() -> Vec<InputDevice> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices.filter_map(|d| describe(&d)).collect()
}

/// The microphone macOS currently treats as the default, so the UI can name it
/// instead of leaving "System default" a mystery.
pub fn default_input_device() -> Option<InputDevice> {
    describe(&cpal::default_host().default_input_device()?)
}

fn describe(device: &cpal::Device) -> Option<InputDevice> {
    let id = device.id().ok()?.to_string();
    let name = device
        .description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| id.clone());
    Some(InputDevice { id, name })
}

impl CaptureService {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let loopback = cmd_tx.clone();
        std::thread::Builder::new()
            .name("speakly-capture".into())
            .spawn(move || capture_thread(cmd_rx, loopback))
            .expect("spawn capture thread");
        Self { cmd_tx }
    }

    /// Open the best available input from `priority` and start streaming mono
    /// f32 chunks into `out`. Returns the sample rate those chunks are in,
    /// which stays fixed for the life of the capture even if the device is
    /// swapped underneath. The `out` sender is dropped when capture stops,
    /// closing the channel.
    pub fn start(&self, out: Sender<Vec<f32>>, priority: Vec<String>) -> Result<u32, String> {
        let (reply_tx, reply_rx) = bounded(1);
        self.cmd_tx
            .send(Cmd::Start {
                out,
                priority,
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

/// What the running capture is set up to do, kept so a device loss can be
/// repaired without involving the caller.
struct Session {
    out: Sender<Vec<f32>>,
    priority: Vec<String>,
    /// Rate promised to the collector at `start`; a replacement device is
    /// converted to it rather than changing it.
    rate: u32,
    name: String,
    generation: u64,
    /// Devices swapped in so far. Capped so a microphone that opens and dies
    /// straight away cannot spin the thread for the rest of the utterance.
    swaps: u8,
}

/// How many mid-recording device swaps one capture may make.
const MAX_SWAPS: u8 = 3;

fn capture_thread(cmd_rx: Receiver<Cmd>, loopback: Sender<Cmd>) {
    // The live stream (and the `out` clone captured by its callback) lives
    // here; dropping it stops CoreAudio callbacks.
    let mut active: Option<cpal::Stream> = None;
    // The master `out` clone lives in the session, so the channel stays open
    // across a device swap and closes only on Stop.
    let mut session: Option<Session> = None;
    let mut generation: u64 = 0;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Start {
                out,
                priority,
                reply,
            } => {
                generation += 1;
                match open_stream(out.clone(), &priority, None, &loopback, generation) {
                    Ok(opened) => {
                        drop(active.replace(opened.stream));
                        let rate = opened.rate;
                        session = Some(Session {
                            out,
                            priority,
                            rate,
                            name: opened.name,
                            generation,
                            swaps: 0,
                        });
                        let _ = reply.send(Ok(rate));
                    }
                    Err(e) => {
                        drop(active.take());
                        session = None;
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Cmd::DeviceLost { generation: from } => {
                let Some(current) = session.as_mut() else {
                    continue;
                };
                if current.generation != from {
                    continue;
                }
                if current.swaps >= MAX_SWAPS {
                    tracing::warn!(
                        "microphone '{}' keeps dropping out — giving up after {MAX_SWAPS} \
                         switches",
                        current.name
                    );
                    continue;
                }
                current.swaps += 1;
                // Release the dead device before re-enumerating, or it can
                // still show up as connected and get picked straight back.
                drop(active.take());
                generation += 1;
                match open_stream(
                    current.out.clone(),
                    &current.priority,
                    Some(current.rate),
                    &loopback,
                    generation,
                ) {
                    Ok(opened) => {
                        tracing::warn!(
                            "microphone '{}' disconnected mid-recording — switched to '{}'",
                            current.name,
                            opened.name
                        );
                        active = Some(opened.stream);
                        current.name = opened.name;
                        current.generation = generation;
                    }
                    Err(e) => tracing::warn!(
                        "microphone '{}' disconnected mid-recording and no replacement \
                         could be opened: {e}",
                        current.name
                    ),
                }
            }
            Cmd::Stop => {
                drop(active.take());
                session = None;
            }
        }
    }
}

struct Opened {
    stream: cpal::Stream,
    rate: u32,
    name: String,
}

/// First connected microphone in `priority`, else the OS default.
fn pick(host: &cpal::Host, priority: &[String]) -> Option<cpal::Device> {
    let connected: Vec<cpal::Device> = host.input_devices().ok()?.collect();
    for want in priority {
        if let Some(device) = connected
            .iter()
            .find(|d| d.id().is_ok_and(|id| id.to_string() == *want))
        {
            return Some(device.clone());
        }
    }
    if !priority.is_empty() {
        // Unplugged headset with nothing else on the list still connected:
        // record from the default rather than failing the dictation outright.
        tracing::warn!(
            "none of the {} preferred microphones are connected — using the system default",
            priority.len()
        );
    }
    host.default_input_device()
}

fn open_stream(
    out: Sender<Vec<f32>>,
    priority: &[String],
    lock_rate: Option<u32>,
    loopback: &Sender<Cmd>,
    generation: u64,
) -> Result<Opened, String> {
    let host = cpal::default_host();
    let device = pick(&host, priority).ok_or("no input device available")?;
    let name = describe(&device).map(|d| d.name).unwrap_or_default();

    let mut supported = device
        .default_input_config()
        .map_err(|e| format!("input config: {e}"))?;
    // On a mid-recording swap the collector was already told a rate, so open
    // the replacement at that rate when it supports it — cheaper and cleaner
    // than converting.
    if let Some(rate) = lock_rate {
        if supported.sample_rate() != rate {
            if let Some(matched) = device.supported_input_configs().ok().and_then(|configs| {
                configs
                    .filter(|c| c.sample_format() == supported.sample_format())
                    .find_map(|c| c.try_with_sample_rate(rate))
            }) {
                supported = matched;
            }
        }
    }

    let native = supported.sample_rate();
    let rate = lock_rate.unwrap_or(native);
    // Last resort when the replacement cannot run at the locked rate: convert
    // in the callback, so the tail of the utterance still lines up with the
    // head instead of playing back at the wrong speed.
    let mut rerate = (native != rate).then(|| {
        tracing::info!("'{name}' runs at {native} Hz; converting to the capture's {rate} Hz");
        Lerp::new(native, rate)
    });

    tracing::info!("recording from '{name}' at {native} Hz");
    let channels = supported.channels() as usize;
    let lost = loopback.clone();
    let lost_name = name.clone();
    let stream = device
        .build_input_stream(
            supported.into(),
            move |data: &[f32], _| {
                let mono: Vec<f32> = if channels == 1 {
                    data.to_vec()
                } else {
                    data.chunks(channels)
                        .map(|f| f.iter().sum::<f32>() / channels as f32)
                        .collect()
                };
                let _ = out.send(match rerate.as_mut() {
                    Some(l) => l.push(&mono),
                    None => mono,
                });
            },
            move |e| match e.kind() {
                ErrorKind::DeviceNotAvailable => {
                    tracing::warn!("capture device '{lost_name}' went away: {e}");
                    let _ = lost.send(Cmd::DeviceLost { generation });
                }
                // The host already rerouted us; the stream stays valid.
                ErrorKind::DeviceChanged => tracing::info!("audio route changed: {e}"),
                _ => tracing::warn!("capture stream error: {e}"),
            },
            None,
        )
        .map_err(|e| format!("build input stream: {e}"))?;
    stream.play().map_err(|e| format!("start stream: {e}"))?;
    Ok(Opened { stream, rate, name })
}

/// Streaming linear resampler. Only ever used to fit a replacement microphone
/// to the rate capture already started at, so a few tenths of a dB of aliasing
/// in the top octave is a fine price for not dropping the rest of a sentence;
/// the utterance's real resampling to 16 kHz still goes through rubato.
struct Lerp {
    /// Input samples consumed per output sample.
    step: f64,
    /// Read cursor in the current buffer. Negative means "still finishing the
    /// gap between the previous buffer's last sample and this one's first".
    pos: f64,
    prev: f32,
}

impl Lerp {
    fn new(from: u32, to: u32) -> Self {
        Self {
            step: from as f64 / to as f64,
            pos: 0.0,
            prev: 0.0,
        }
    }

    fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let n = input.len() as f64;
        let mut out = Vec::with_capacity((n / self.step).ceil() as usize + 1);
        // Stop one sample short of the end: interpolating the final gap needs
        // the next buffer's first sample, which arrives as `prev` next time.
        while self.pos < n - 1.0 {
            let base = self.pos.floor();
            let frac = (self.pos - base) as f32;
            let i = base as isize;
            let a = if i < 0 { self.prev } else { input[i as usize] };
            let b = input[(i + 1) as usize];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.prev = input[input.len() - 1];
        self.pos -= n;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Lerp;

    #[test]
    fn halving_the_rate_halves_the_sample_count() {
        let mut lerp = Lerp::new(48_000, 24_000);
        let total: usize = (0..4)
            .map(|_| lerp.push(&vec![0.5; 480]).len())
            .sum();
        // 1920 in at 2:1, within one sample of 960 out across the buffers.
        assert!((total as i64 - 960).abs() <= 1, "got {total}");
    }

    #[test]
    fn a_ramp_stays_monotonic_across_buffer_edges() {
        let mut lerp = Lerp::new(44_100, 48_000);
        let mut last = f32::NEG_INFINITY;
        let mut count = 0;
        for buffer in 0..3 {
            let input: Vec<f32> = (0..256).map(|i| (buffer * 256 + i) as f32).collect();
            for sample in lerp.push(&input) {
                assert!(sample >= last, "went backwards at {count}: {sample} < {last}");
                last = sample;
                count += 1;
            }
        }
        // Upsampling produces more samples than it consumed.
        assert!(count > 768, "got {count}");
    }
}
