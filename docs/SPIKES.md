# P0 Spike Results

Machine: Apple M4 Max, macOS. whisper-rs 0.16.0 (whisper-rs-sys 0.15.0), Metal, greedy decoding, 4 threads, release build.

## Spike A — warm decode latency (PASSED)

`cargo run --release -p speakly-engine --bin spike_a -- <model> <wav> [--language he] [--audio-ctx N]`

| Model | Audio | audio_ctx | Warm decode | Realtime | Transcript |
|---|---|---|---|---|---|
| large-v3-turbo (f16) | en-jfk.wav 11.0 s | full (1500) | ~500–550 ms | ~20× | perfect |
| large-v3-turbo (f16) | en-jfk.wav 11.0 s | 704 | **~150–260 ms** | ~70× | identical to full |
| ivrit large-v3-turbo (f16) | he-dictation.wav 8.3 s | full (1500) | ~585–710 ms | ~13× | character-perfect Hebrew incl. punctuation |
| ivrit large-v3-turbo (f16) | he-dictation.wav 8.3 s | 576 | **~185–250 ms** | ~40× | identical to full |

- Model load: ~8.3 s cold (first mmap + Metal init), ~560 ms with warm OS file cache → the warm-persistent-context architecture is what makes dictation instant.
- `audio_ctx` scaling (`ceil(len/30 s × 1500) + 128`) gives a ~3–3.5× decode speedup with no transcript change on either model, including the ivrit fine-tune. First A/B passed; validate on real-microphone Hebrew before enabling by default for ivrit.
- Hebrew fixture is Carmit TTS (clean audio); real-mic accuracy validation happens during P1 dogfooding.

## Spike C — VAD API availability (PASSED, resolved early)

whisper-rs 0.16.0 exposes both VAD paths we need, safe-API level:
- Integrated file-job VAD: `FullParams::enable_vad`, `set_vad_model_path`, `set_vad_params` (whisper.cpp remaps timestamps to the original timeline internally).
- Standalone live gating: `WhisperVadContext` (`src/whisper_vad.rs`) wrapping `whisper_vad_detect_speech` / segments.

No sherpa-rs VAD fallback needed. Silero VAD ggml model (~2 MB) to be added to the model registry.

## Spike B — capture chain (pending)

cpal → SPSC ring → rubato 16 k mono resample → wav dump verification. Next up; low risk.
