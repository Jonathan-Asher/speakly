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
//!
//! One entry can be connected yet unusable: the MacBook's own microphone is
//! hardware-disabled whenever the lid is closed (clamshell mode, external
//! display), but it still enumerates through CoreAudio, so "is it in the
//! device list" cannot distinguish it from a working mic. Selecting it anyway
//! records perfect silence, so clamshell state is checked before picking.

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

/// Is this input one of the Mac's own microphones? cpal does not expose the
/// transport, so the name is the signal. Covers the Apple Silicon names
/// ("MacBook Pro Microphone", "MacBook Air Microphone") and the older
/// "Internal/Built-in Microphone". Conservative on purpose: a USB or Bluetooth
/// box that happens to embed these words must never be skipped.
fn is_builtin_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("macbook") || n.contains("internal microphone") || n.contains("built-in microphone")
}

/// Would this device be selectable with the lid in this state? The one rule
/// everything else composes from, kept pure so it is directly testable: a
/// closed lid disables only the built-in microphone, never anything plugged
/// or paired in.
fn is_selectable(builtin: bool, lid_closed: bool) -> bool {
    !(builtin && lid_closed)
}

/// True when the MacBook lid is closed (clamshell mode).
///
/// When closed, macOS hardware-disables the internal microphone — but the
/// device still enumerates, and opening it succeeds while delivering silence.
/// Only the IORegistry knows the truth: the power-management root domain
/// publishes `AppleClamshellState`. Read via `ioreg` rather than IOKit
/// bindings to avoid a new dependency for one property; a spawn costs tens of
/// milliseconds, and this is consulted on device selection and failover —
/// never per audio callback — with a short cache so a burst of re-picks
/// cannot spam it.
///
/// Fail-open: an unreadable registry answers "open" and preserves the old
/// behavior, which is wrong only on a closed lid, not everywhere.
#[cfg(target_os = "macos")]
fn lid_closed() -> bool {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// The lid does not flap mid-second; one cached answer per 2s is plenty.
    const TTL: Duration = Duration::from_secs(2);
    static CACHE: std::sync::OnceLock<Mutex<Option<(Instant, bool)>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));

    let mut cached = cache.lock().unwrap();
    if let Some((read_at, closed)) = *cached {
        if read_at.elapsed() < TTL {
            return closed;
        }
    }

    let output = std::process::Command::new("ioreg")
        .args(["-r", "-k", "AppleClamshellState", "-d", "1"])
        .output();
    let parsed = output
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| clamshell_from_ioreg(&String::from_utf8_lossy(&out.stdout)));
    if let Some(closed) = parsed {
        *cached = Some((Instant::now(), closed));
    }
    parsed.unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn lid_closed() -> bool {
    // No lid on this platform, no guard needed.
    false
}

/// Extract `AppleClamshellState` from `ioreg -r -k AppleClamshellState -d 1`
/// output. Matches the exact key — the same node also carries
/// `AppleClamshellCausesSleep`, which must not be mistaken for it. None means
/// the property is absent or unreadable.
#[cfg(target_os = "macos")]
fn clamshell_from_ioreg(output: &str) -> Option<bool> {
    output.lines().find_map(|line| {
        let value = line.split("\"AppleClamshellState\" = ").nth(1)?.trim();
        match value {
            "Yes" => Some(true),
            "No" => Some(false),
            _ => None,
        }
    })
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

fn capture_thread(cmd_rx: Receiver<Cmd>) {
    // Device losses are reported by the stream error callbacks on their own
    // channel rather than back through `cmd_rx`. Holding a `Cmd` sender in
    // here would stop `cmd_rx` from ever disconnecting, and that disconnect is
    // exactly what ends this thread when its `CaptureService` is dropped.
    let (lost_tx, lost_rx) = unbounded::<u64>();
    let mut capture = Capture {
        active: None,
        session: None,
        generation: 0,
        lost_tx,
    };

    loop {
        crossbeam_channel::select! {
            recv(cmd_rx) -> cmd => match cmd {
                Ok(Cmd::Start { out, priority, reply }) => capture.start(out, priority, reply),
                Ok(Cmd::Stop) => capture.stop(),
                // The service is gone, so no further command can arrive.
                Err(_) => break,
            },
            recv(lost_rx) -> generation => if let Ok(g) = generation { capture.failover(g) },
        }
    }
}

/// Mutable state of the capture thread, kept in one place so each `select!`
/// arm is a single call.
struct Capture {
    /// The live stream (and the `out` clone captured by its callback);
    /// dropping it stops CoreAudio callbacks.
    active: Option<cpal::Stream>,
    /// The master `out` clone lives here, so the channel stays open across a
    /// device swap and closes only on stop.
    session: Option<Session>,
    generation: u64,
    lost_tx: Sender<u64>,
}

impl Capture {
    fn start(
        &mut self,
        out: Sender<Vec<f32>>,
        priority: Vec<String>,
        reply: Sender<Result<u32, String>>,
    ) {
        self.generation += 1;
        match open_stream(out.clone(), &priority, None, &self.lost_tx, self.generation) {
            Ok(opened) => {
                drop(self.active.replace(opened.stream));
                let rate = opened.rate;
                self.session = Some(Session {
                    out,
                    priority,
                    rate,
                    name: opened.name,
                    generation: self.generation,
                    swaps: 0,
                });
                let _ = reply.send(Ok(rate));
            }
            Err(e) => {
                self.stop();
                let _ = reply.send(Err(e));
            }
        }
    }

    fn stop(&mut self) {
        drop(self.active.take());
        self.session = None;
    }

    /// Reopen on the next available microphone after the live one vanished.
    /// `from` is the generation of the stream that complained, so a late error
    /// from one already replaced cannot trigger a second switch.
    fn failover(&mut self, from: u64) {
        let Some(current) = self.session.as_mut() else {
            return;
        };
        if current.generation != from {
            return;
        }
        if current.swaps >= MAX_SWAPS {
            tracing::warn!(
                "microphone '{}' keeps dropping out — giving up after {MAX_SWAPS} switches",
                current.name
            );
            return;
        }
        current.swaps += 1;
        // Release the dead device before re-enumerating, or it can still show
        // up as connected and get picked straight back.
        drop(self.active.take());
        self.generation += 1;
        match open_stream(
            current.out.clone(),
            &current.priority,
            Some(current.rate),
            &self.lost_tx,
            self.generation,
        ) {
            Ok(opened) => {
                tracing::warn!(
                    "microphone '{}' disconnected mid-recording — switched to '{}'",
                    current.name,
                    opened.name
                );
                self.active = Some(opened.stream);
                current.name = opened.name;
                current.generation = self.generation;
            }
            Err(e) => tracing::warn!(
                "microphone '{}' disconnected mid-recording and no replacement could be \
                 opened: {e}",
                current.name
            ),
        }
    }
}

struct Opened {
    stream: cpal::Stream,
    rate: u32,
    name: String,
}

/// First connected microphone in `priority`, else the OS default.
///
/// The clamshell guard runs here (and therefore on failover too, which re-enters
/// through `open_stream` → `pick`): with the lid closed the built-in microphone
/// is hardware-disabled yet still enumerates, so it is skipped exactly like a
/// disconnected entry and selection falls through to the next preference.
/// A `Result` rather than an `Option` so the refusal can say WHY nothing was
/// selectable instead of surfacing as a generic "no input device available".
fn pick(host: &cpal::Host, priority: &[String]) -> Result<cpal::Device, String> {
    let clamshell = lid_closed();
    let connected: Vec<cpal::Device> = host
        .input_devices()
        .map_err(|e| format!("enumerate input devices: {e}"))?
        .collect();
    for want in priority {
        if let Some(device) = connected
            .iter()
            .find(|d| d.id().is_ok_and(|id| id.to_string() == *want))
        {
            if let Some(name) = describe(device).map(|d| d.name) {
                if !is_selectable(is_builtin_name(&name), clamshell) {
                    tracing::warn!(
                        "lid is closed — '{name}' is disabled in clamshell mode, skipping it"
                    );
                    continue;
                }
            }
            return Ok(device.clone());
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
    let default = host
        .default_input_device()
        .ok_or("no input device available")?;
    // The default can be the built-in itself (no priority list, or every entry
    // skipped above). Recording from it in clamshell means silence, so refuse
    // and fail the dictation with a reason instead.
    let name = describe(&default).map(|d| d.name).unwrap_or_default();
    if !is_selectable(is_builtin_name(&name), clamshell) {
        return Err(format!(
            "the lid is closed — the built-in microphone '{name}' is disabled in clamshell mode"
        ));
    }
    Ok(default)
}

fn open_stream(
    out: Sender<Vec<f32>>,
    priority: &[String],
    lock_rate: Option<u32>,
    lost_tx: &Sender<u64>,
    generation: u64,
) -> Result<Opened, String> {
    let host = cpal::default_host();
    let device = pick(&host, priority)?;
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
    let lost = lost_tx.clone();
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
                    let _ = lost.send(generation);
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
    use super::{capture_thread, Cmd, Lerp};
    use crossbeam_channel::unbounded;

    /// The thread must end when its `CaptureService` goes away. It once kept a
    /// `Cmd` sender of its own so device-loss reports could loop back, which
    /// meant `cmd_rx` never disconnected and every capture thread ever spawned
    /// stayed alive — one leaked per microphone probe.
    #[test]
    fn the_thread_ends_when_its_service_is_dropped() {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let thread = std::thread::spawn(move || capture_thread(cmd_rx));
        drop(cmd_tx);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !thread.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "capture thread outlived its service"
            );
            std::thread::yield_now();
        }
        thread.join().unwrap();
    }

    #[test]
    fn a_closed_lid_disables_only_the_builtin() {
        use super::is_selectable;
        // Closed lid: the built-in mic is hardware-disabled — skip it.
        assert!(!is_selectable(true, true));
        // Closed lid: everything plugged or paired in still works.
        assert!(is_selectable(false, true));
        // Open lid: the built-in mic is a perfectly good microphone.
        assert!(is_selectable(true, false));
        assert!(is_selectable(false, false));
    }

    #[test]
    fn builtin_detection_names_the_macs_own_microphones() {
        use super::is_builtin_name;
        assert!(is_builtin_name("MacBook Pro Microphone"));
        assert!(is_builtin_name("MacBook Air Microphone"));
        assert!(is_builtin_name("MacBook Microphone"));
        assert!(is_builtin_name("Internal Microphone"));
        assert!(is_builtin_name("Built-in Microphone"));
        // External hardware must never match, whatever its name.
        assert!(!is_builtin_name("DJI Mic Mini 2-CE73A4"));
        assert!(!is_builtin_name("WH-1000XM6"));
        assert!(!is_builtin_name("Wireless Mic Rx"));
        assert!(!is_builtin_name("Jonathan’s iPhone Microphone"));
        assert!(!is_builtin_name(""));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn ioreg_output_parses_into_a_lid_state() {
        use super::clamshell_from_ioreg;
        let open = "+-o IOPMrootDomain  <class IOPMrootDomain>\n";
        let yes = "      \"AppleClamshellCausesSleep\" = No\n      \"AppleClamshellState\" = Yes\n";
        // Only the exact key counts — the CausesSleep variant must not leak in.
        assert_eq!(clamshell_from_ioreg(&format!("{open}{yes}")), Some(true));
        let no = "      \"AppleClamshellCausesSleep\" = Yes\n      \"AppleClamshellState\" = No\n";
        assert_eq!(clamshell_from_ioreg(&format!("{open}{no}")), Some(false));
        // Property absent (desktop Mac, or a registry layout change): unreadable,
        // which callers must treat as "lid open".
        assert_eq!(clamshell_from_ioreg(open), None);
        assert_eq!(clamshell_from_ioreg(""), None);
        // A value we do not recognize is not a yes.
        let weird = "      \"AppleClamshellState\" = 12\n";
        assert_eq!(clamshell_from_ioreg(weird), None);
    }

    #[test]
    fn halving_the_rate_halves_the_sample_count() {
        let mut lerp = Lerp::new(48_000, 24_000);
        let total: usize = (0..4).map(|_| lerp.push(&vec![0.5; 480]).len()).sum();
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
                assert!(
                    sample >= last,
                    "went backwards at {count}: {sample} < {last}"
                );
                last = sample;
                count += 1;
            }
        }
        // Upsampling produces more samples than it consumed.
        assert!(count > 768, "got {count}");
    }
}
