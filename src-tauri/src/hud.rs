//! The recording pill: a small transparent always-on-top window shown while
//! dictating, positioned bottom-center of the monitor under the cursor. The
//! window is made non-activating (it can never become key and steal the paste
//! target's focus) and ignores the mouse.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

pub const HUD_LABEL: &str = "hud";
const WIDTH: f64 = 320.0;
const HEIGHT: f64 = 76.0;
const BOTTOM_MARGIN: f64 = 96.0;

pub fn ensure(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(HUD_LABEL).is_some() {
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, HUD_LABEL, WebviewUrl::App("hud.html".into()))
        .title("Speakly")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .accept_first_mouse(false)
        .skip_taskbar(true)
        .focused(false)
        .resizable(false)
        .visible(false)
        .build()?;
    let _ = window.set_ignore_cursor_events(true);
    make_non_activating(&window);
    Ok(())
}

/// Prevent the HUD from ever becoming the key window. Runtime re-classing to
/// NSPanel is impossible and a git-dependency on a panel crate is a worse
/// trade, so this uses NSWindow's private `_setPreventsActivation:` (fair game
/// with `macOSPrivateApi` already on) plus full-screen-auxiliary + stationary
/// collection behavior so the pill also shows over full-screen apps.
#[cfg(target_os = "macos")]
fn make_non_activating(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ptr) = window.ns_window() else { return };
    let ns = ptr as *mut AnyObject;
    unsafe {
        let _: () = msg_send![&mut *ns, _setPreventsActivation: true];
        const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;
        const STATIONARY: u64 = 1 << 4;
        let behavior: u64 = msg_send![&*ns, collectionBehavior];
        let _: () = msg_send![&mut *ns, setCollectionBehavior: behavior | FULL_SCREEN_AUXILIARY | STATIONARY];
    }
}

#[cfg(not(target_os = "macos"))]
fn make_non_activating(_window: &tauri::WebviewWindow) {}

/// Debug probe: is the HUD currently the key window? Must be false whenever
/// the pill is visible — asserted manually during QA via the command.
#[cfg(target_os = "macos")]
pub fn is_key_window(app: &AppHandle) -> bool {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        return false;
    };
    let Ok(ptr) = window.ns_window() else {
        return false;
    };
    unsafe {
        let is_key: bool = msg_send![&*(ptr as *mut AnyObject), isKeyWindow];
        is_key
    }
}

pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        return;
    };
    position_on_cursor_monitor(app, &window);
    let _ = window.show();
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(HUD_LABEL) {
        let _ = window.hide();
    }
}

fn position_on_cursor_monitor(app: &AppHandle, window: &tauri::WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|pos| app.monitor_from_point(pos.x, pos.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let scale = monitor.scale_factor();
    let mpos = monitor.position();
    let msize = monitor.size();
    let w = WIDTH * scale;
    let h = HEIGHT * scale;
    let x = mpos.x as f64 + (msize.width as f64 - w) / 2.0;
    let y = mpos.y as f64 + msize.height as f64 - h - BOTTOM_MARGIN * scale;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
