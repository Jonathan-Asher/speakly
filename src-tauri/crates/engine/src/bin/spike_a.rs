//! Spike A — warm-decode latency bench (the P0 technical gate).
//!
//! Measures, on this machine: model load time, first (warmup, Metal shader
//! compile) decode, and steady-state warm decode times for a wav file, with an
//! optional `audio_ctx` override to quantify the short-utterance speedup.
//!
//! Usage:
//!   spike-a <model.bin> <audio.wav> [--language he] [--runs 5] [--audio-ctx N]
//!           [--threads N] [--no-gpu]

use std::time::Instant;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

struct Args {
    model: String,
    wav: String,
    language: String,
    runs: usize,
    audio_ctx: Option<i32>,
    threads: i32,
    use_gpu: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        model: String::new(),
        wav: String::new(),
        language: "en".into(),
        runs: 5,
        audio_ctx: None,
        threads: 4,
        use_gpu: true,
    };
    let mut positional = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--language" => args.language = it.next().expect("--language value"),
            "--runs" => args.runs = it.next().expect("--runs value").parse().expect("runs"),
            "--audio-ctx" => {
                args.audio_ctx = Some(it.next().expect("--audio-ctx value").parse().expect("n"))
            }
            "--threads" => args.threads = it.next().expect("--threads value").parse().expect("n"),
            "--no-gpu" => args.use_gpu = false,
            other => positional.push(other.to_string()),
        }
    }
    if positional.len() != 2 {
        eprintln!("usage: spike-a <model.bin> <audio.wav> [--language he] [--runs 5] [--audio-ctx N] [--threads N] [--no-gpu]");
        std::process::exit(2);
    }
    args.model = positional.remove(0);
    args.wav = positional.remove(0);
    args
}

/// Load a wav as 16 kHz mono f32. The spike only accepts what whisper needs;
/// resampling arbitrary input is Spike B territory.
fn load_wav_16k_mono(path: &str) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "spike expects a 16 kHz wav");
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.expect("sample") as f32 / max)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.expect("sample"))
            .collect(),
    };
    if spec.channels == 1 {
        samples
    } else {
        samples
            .chunks(spec.channels as usize)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect()
    }
}

fn main() {
    let args = parse_args();
    let audio = load_wav_16k_mono(&args.wav);
    let audio_secs = audio.len() as f32 / 16_000.0;
    println!(
        "model={} wav={} ({:.1}s) lang={} gpu={} threads={} audio_ctx={:?}",
        args.model, args.wav, audio_secs, args.language, args.use_gpu, args.threads, args.audio_ctx
    );

    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.use_gpu(args.use_gpu);
    ctx_params.flash_attn(true);

    let t0 = Instant::now();
    let ctx = WhisperContext::new_with_params(&args.model, ctx_params).expect("load model");
    println!("model load: {} ms", t0.elapsed().as_millis());

    let mut last_text = String::new();
    for run in 0..=args.runs {
        let mut state = ctx.create_state().expect("create state");
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&args.language));
        params.set_n_threads(args.threads);
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_nst(true);
        if let Some(ac) = args.audio_ctx {
            params.set_audio_ctx(ac);
        }

        let t = Instant::now();
        state.full(params, &audio).expect("decode");
        let ms = t.elapsed().as_millis();

        let n = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n {
            if let Some(seg) = state.get_segment(i) {
                text.push_str(&seg.to_str_lossy().expect("segment text"));
            }
        }
        last_text = text.trim().to_string();

        let label = if run == 0 { "warmup" } else { "warm  " };
        println!(
            "run {:>2} [{}]: {:>5} ms  ({:.1}x realtime)",
            run,
            label,
            ms,
            audio_secs / (ms as f32 / 1000.0)
        );
    }
    println!("transcript: {last_text}");
}
