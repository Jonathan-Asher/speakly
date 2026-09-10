# Diagnostics

Everything Speakly does to a dictation leaves a line in the log, so a report
like "it cut my sentence" or "the pill vanished" can be answered from evidence
instead of guesswork.

## Getting the log

**Settings → Diagnostics → Reveal in Finder**, or:

```
~/Library/Logs/Speakly/speakly.log.YYYY-MM-DD
```

Rotated daily. Default level is `info`; every line below is `info` or louder,
so nothing here needs a special build. To turn up the volume for a session:

```
RUST_LOG=debug /Applications/Speakly.app/Contents/MacOS/speakly
```

Running the binary directly like this also puts the log on stderr, which is
the fastest way to watch a reproduction as it happens.

## Reading one dictation

A healthy dictation writes, in order:

| Line | Means |
|---|---|
| `profile <id> on <hotkey>` | at startup: the hotkey registered |
| `side-specific hotkey — <id>` | the tap matched a sided combination |
| `recording from '<name>' at <n> Hz` | which microphone actually opened |
| `pill → screen …, placed at x,y` | where the recording indicator went |
| `first partial decode <ms> ms → …` | the live preview is running |
| `dictation finalised: …` | the summary line — see below |
| `ai stage (refine+translate) via <provider>: <before> → <after> chars` | post-processing |
| `pasting into pid <n>` / `paste delivered` | where the text landed |

### The `dictation finalised` line

This is the one that settles truncation reports. It carries the audio length,
what the live preview had accumulated, and what the final decode produced:

```
dictation finalised: 62.4s audio | preview 58.1s/494 chars | decoded 62.4s → 511 chars | decode 3820 ms
```

Read it as: **`decoded` is what gets pasted.** If `decoded` is much shorter
than `audio`, the decode dropped material. If `decoded` is fine but the pasted
text is short, the loss happened downstream — check the `ai stage` line, whose
`before → after` character counts show exactly what refine/translate did to it.

Two truncation bugs were found this way and both are guarded now: committed
segments no longer advance the offset on empty text, and the final decode
always covers the whole utterance rather than trusting chunk decodes.

## Microphones

Selection is an ordered list (Settings → Microphone). The first connected
entry wins; if none are connected, the OS default is used.

| Line | Means |
|---|---|
| `recording from '<name>' at <n> Hz` | the device that won |
| `none of the N preferred microphones are connected — using the system default` | the whole list was unreachable |
| `capture device '<name>' went away` | CoreAudio reported the device gone |
| `microphone '<a>' disconnected mid-recording — switched to '<b>'` | failover succeeded, the utterance continues |
| `'<name>' runs at <n> Hz; converting to the capture's <m> Hz` | the replacement could not match the original rate, so samples are converted |
| `audio route changed` | the host rerouted us itself; no action needed |
| `microphone '<name>' keeps dropping out — giving up after 3 switches` | failover is capped, to stop a flapping device spinning the capture thread |

The sample rate is fixed when capture starts and never changes mid-utterance —
a replacement device is either opened at that rate or converted to it, because
changing rates halfway would make everything after the switch play back at the
wrong speed.

## The recording indicator

The pill is placed in Cocoa screen coordinates: points, one origin for all
displays. It logs where it went:

```
pill → screen 0,0 1440x900, placed at 480,96 (cursor 582,383)
```

If it is ever invisible again, that line says which display was chosen and
whether the position is inside it. Two failure signatures to look for:

- `no NSScreen contains the pointer at x,y (N screens)` — AppKit could not
  match the cursor to a display, so placement fell back to Tauri's monitor API.
- `no monitor contains the cursor at x,y` — the fallback could not either, so
  the pill went to the primary display.

Neither should happen. They exist because the original bug was exactly this:
placement went through Tauri's `PhysicalPosition`, whose per-monitor rects are
each derived from that monitor's own scale factor. On a mixed-DPI setup those
rects neither tile nor agree with the physical cursor position, so
`monitor_from_point` silently matched nothing and a position computed for one
display could land the window off every screen.

## Permissions

| Line | Means |
|---|---|
| `hotkey registration: <name>: '<hotkey>' needs the Accessibility permission` | sided hotkeys are inert until granted |
| `modifier tap install failed (Accessibility missing?)` | the event tap could not install, so Esc-to-cancel and side-specific hotkeys are off |

Permission grants are keyed to the app's **designated requirement**, not to a
particular build. Since releases are signed with the Developer ID identity, the
requirement is hash-independent and grants survive updates:

```
identifier "com.speakly.app" and anchor apple generic
  and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */
  and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */
  and certificate leaf[subject.OU] = "3L92BZK46V"
```

Note it names the certificate, not a code hash — which is why a rebuild keeps
the grants.

Check a build with `codesign -d -r- /Applications/Speakly.app`. If that comes
back ad-hoc (`cdhash`-based), permissions will be re-asked on every update —
which is why `scripts/build-local.sh` refuses to build rather than fall back
to ad-hoc signing.

## Updates

`update available: <version>` at startup means the updater reached
`latest.json` and found something newer. Update archives are signed with a
minisign key whose public half is in `tauri.conf.json`; only the release
workflow holds the private half, so a locally built `.tar.gz` will not install
as an update (and the local build prints a key-mismatch warning saying so).
