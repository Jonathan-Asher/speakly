//! Permission status + recovery plumbing for onboarding and Settings. The mic
//! TCC prompt itself is triggered naturally by a short capture probe; deep
//! links open the right System Settings pane for denied states.

use std::sync::Arc;

use serde_json::{json, Value};
use speakly_engine::Engine;
use tauri::State;

/// TCC state for the microphone, read via AVFoundation.
pub fn microphone_status() -> &'static str {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    unsafe {
        let Some(media) = AVMediaTypeAudio else {
            return "unknown";
        };
        let status = AVCaptureDevice::authorizationStatusForMediaType(media);
        if status == AVAuthorizationStatus::Authorized {
            "granted"
        } else if status == AVAuthorizationStatus::Denied
            || status == AVAuthorizationStatus::Restricted
        {
            "denied"
        } else if status == AVAuthorizationStatus::NotDetermined {
            "undetermined"
        } else {
            "unknown"
        }
    }
}

#[tauri::command]
pub fn check_permissions() -> Value {
    json!({
        "microphone": microphone_status(),
        "accessibility": crate::paste::accessibility_trusted(),
    })
}

/// Open the default input for ~half a second. From an app process this fires
/// the system microphone prompt on first use — exactly what onboarding wants
/// from a user-initiated button.
#[tauri::command]
pub async fn probe_microphone(engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    let engine = Arc::clone(&engine);
    tauri::async_runtime::spawn_blocking(move || engine.mic_probe())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn open_privacy_pane(pane: String) -> Result<(), String> {
    let url = match pane.as_str() {
        "microphone" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        other => return Err(format!("unknown privacy pane: {other}")),
    };
    tauri_plugin_opener::open_url(url, None::<String>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn microphone_status_returns_meaningful_value() {
        let s = super::microphone_status();
        // A fresh test process should be undetermined (or denied on CI);
        // "unknown" means the AVFoundation path itself is broken.
        assert_ne!(s, "unknown", "AVFoundation status read failed");
    }
}
