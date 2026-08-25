# [2.1.0](https://github.com/Jonathan-Asher/speakly/compare/v2.0.1...v2.1.0) (2026-08-25)


### Features

* toggle mode for bare-modifier hotkeys ([89f124c](https://github.com/Jonathan-Asher/speakly/commit/89f124cf184f4a585bf8101175eae22f057b691c))

## [2.0.1](https://github.com/Jonathan-Asher/speakly/compare/v2.0.0...v2.0.1) (2026-08-25)


### Bug Fixes

* toggle-mode hotkeys and onboarding relapse after profile edits ([97751e8](https://github.com/Jonathan-Asher/speakly/commit/97751e8e0a547fe73b7cc16a8a656262db8053e5))

## [1.0.2](https://github.com/Jonathan-Asher/speakly/compare/v1.0.1...v1.0.2) (2026-08-25)


### Bug Fixes

* main-thread deadlock on dictation start ([f811515](https://github.com/Jonathan-Asher/speakly/commit/f811515698641e8847df19dc5c79a920e7664640))

## [1.0.1](https://github.com/Jonathan-Asher/speakly/compare/v1.0.0...v1.0.1) (2026-08-25)


### Bug Fixes

* mic permission prompt reliability + local signed builds ([1e55d0d](https://github.com/Jonathan-Asher/speakly/commit/1e55d0dd361eb1b964cdd16d6100f76e434abad8))

# 1.0.0 (2026-08-25)


### Bug Fixes

* **ci:** stage the meeting sidecar before cargo steps (externalBin validation) ([89dfb98](https://github.com/Jonathan-Asher/speakly/commit/89dfb98d3e47343782446927198e37d31b172fb3))
* clean residue gate — managed model paths only, exclude workflows from grep ([0561cf2](https://github.com/Jonathan-Asher/speakly/commit/0561cf2d9bd88389bf7593070702ed45bee450ab))
* **diarize:** statically link sherpa-onnx/onnxruntime — bundled app crashed on missing dylib ([c76a886](https://github.com/Jonathan-Asher/speakly/commit/c76a886ebed55ab0abc802186022e73b03be1783))
* dictation-release crash and abort-on-quit ([c4fc23d](https://github.com/Jonathan-Asher/speakly/commit/c4fc23dbf4dfeb244d4f46e9e94d391313ea6f7c))
* **models:** hidden flag on diarization registry entries post-merge ([81957fc](https://github.com/Jonathan-Asher/speakly/commit/81957fc2dfa2edcca1822b9aeb9c730bfc739212))


### Features

* **diarize:** speaker identification — sherpa-onnx pipeline, merge, files+meetings, rename UI ([d790fca](https://github.com/Jonathan-Asher/speakly/commit/d790fca338b70dc46417cfb941dfee5918a69a59))
* **engine:** streaming dictation — live partials, Silero VAD gate, silence trim ([3dc9683](https://github.com/Jonathan-Asher/speakly/commit/3dc9683f998d9b08b737a77cf873a3dbe951e97b))
* **files:** file transcription — symphonia decode, prioritized job queue, exports, Files view ([123ff44](https://github.com/Jonathan-Asher/speakly/commit/123ff44afa48c9abf3b24f6d21a6bdf4f1430edb))
* **meetings:** system-audio capture via ScreenCaptureKit sidecar with live transcript ([a2c8442](https://github.com/Jonathan-Asher/speakly/commit/a2c84422261f5642d9cfeddeac75beac0860f723))
* **models:** model manager — registry, resumable downloads, Models UI ([a57ef30](https://github.com/Jonathan-Asher/speakly/commit/a57ef304f0160ca220d03c743809dd4b8382da68))
* onboarding wizard, Settings section, permissions plumbing, retention purge ([cafdba4](https://github.com/Jonathan-Asher/speakly/commit/cafdba46977ed6b3dc63426082e7c2e3cb80fb9c))
* P1 dictation vertical slice — hold-to-talk, warm decode, paste, HUD, tray ([676bfd2](https://github.com/Jonathan-Asher/speakly/commit/676bfd2aea3e99577174229b0d53e76f731a56d7))
* polish pack — docx/pdf exports, history segments+filters, non-activating HUD, modifier-hold hotkeys ([c66f42d](https://github.com/Jonathan-Asher/speakly/commit/c66f42d5b222d27ada43816c2a840c72d890520c))
* **profiles:** profile CRUD + hotkey recorder + tray popover ([2f836aa](https://github.com/Jonathan-Asher/speakly/commit/2f836aace36433827e97195bdc01ae8a5a989d3d))
* **release:** auto-updater, signed release pipeline, diagnostics, acknowledgements ([faecfdc](https://github.com/Jonathan-Asher/speakly/commit/faecfdc764ebd0cfae52127edc476c1752ba1579))
* Speakly 2.0 foundation — Tauri 2 workspace, Rust STT engine, P0 spikes ([f0128ae](https://github.com/Jonathan-Asher/speakly/commit/f0128ae1d15b8892d4d1d1e429d87c9b417cd90c))
* SQLite transcript history — FTS5 Hebrew search, dictation persistence, History view ([48bf8e8](https://github.com/Jonathan-Asher/speakly/commit/48bf8e86e0d0b2b86534a46416569217a9fc1b38))
* translation stage + Keychain — the He→En dictation flow ([0dc4851](https://github.com/Jonathan-Asher/speakly/commit/0dc4851b71daa01c082edb7f743faecccfdc06c5))
