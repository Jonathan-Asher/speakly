# Speakly

**Speech to text, instantly.** Private, on-device dictation and transcription for macOS.

- **Push-to-talk dictation** — hold a hotkey, speak, release: your words are pasted at the cursor in any app.
- **Hebrew and English, first-class** — including dictation profiles that transcribe Hebrew and paste the English translation.
- **File transcription** — drag in audio or video, get timestamped transcripts with subtitle and document exports.
- **Meeting capture** *(coming)* — transcribe any app's audio together with your mic, with speaker labels.
- **Private by design** — audio never leaves your Mac. No accounts, no telemetry. Translation, if you enable it, sends only text to the provider you choose.

Powered by a native Rust engine with GPU-accelerated on-device models. Apple Silicon, macOS 13+.

## Development

```bash
pnpm install
pnpm tauri dev
```

The Rust engine lives in `src-tauri/crates/engine`; the app shell in `src-tauri/src`; the React UI in `src/`. Engine benches: `docs/SPIKES.md`.

## License

MIT © Jonathan Asher. Open-source components are listed in Settings → About → Acknowledgements.
