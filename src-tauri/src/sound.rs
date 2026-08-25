//! Subtle audio feedback for dictation start/finish. Spawns the system
//! `afplay` detached — no audio stack in-process, and the system sounds
//! respect the user's alert volume.

use tauri::{AppHandle, Manager};

use crate::settings::SettingsState;

#[derive(Clone, Copy)]
pub enum Cue {
    Start,
    Done,
}

pub fn play(app: &AppHandle, cue: Cue) {
    // try_lock: never block an event-emission path on the settings mutex —
    // under contention (or re-entry) skipping the cue is always correct.
    let enabled = app
        .try_state::<SettingsState>()
        .and_then(|s| s.0.try_lock().ok().map(|g| g.general.sound_feedback))
        .unwrap_or(false);
    if !enabled {
        return;
    }
    let file = match cue {
        Cue::Start => "/System/Library/Sounds/Pop.aiff",
        Cue::Done => "/System/Library/Sounds/Tink.aiff",
    };
    let _ = std::process::Command::new("afplay")
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
