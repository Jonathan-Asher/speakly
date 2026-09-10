//! The recording pill: a small transparent always-on-top window shown while
//! dictating, positioned bottom-center of the monitor under the cursor. The
//! window is made non-activating (it can never become key and steal the paste
//! target's focus) and ignores the mouse.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

pub const HUD_LABEL: &str = "hud";
const WIDTH: f64 = 480.0;
const HEIGHT: f64 = 76.0;
const BOTTOM_MARGIN: f64 = 96.0;
/// Clearance kept above the Dock when it sits along the bottom edge.
const DOCK_GAP: f64 = 8.0;

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
    // Rebuild if the window went away — a pill that silently stops appearing
    // is worse than one that costs a few ms to recreate.
    if let Err(e) = ensure(app) {
        tracing::warn!("could not create the recording pill: {e}");
    }
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        tracing::warn!("recording pill window is missing — nothing to show");
        return;
    };
    // Re-assert: a space switch or another app going full-screen can leave the
    // pill ordered below whatever is in front.
    let _ = window.set_always_on_top(true);
    place(app, &window);
    let _ = window.show();
    // Re-apply once visible: a hidden window can ignore a move, and on macOS
    // the frame only settles onto the target display after the window is
    // ordered in.
    place(app, &window);
}

/// Put the pill bottom-center of the display under the pointer, preferring the
/// AppKit path and falling back to Tauri's monitor API if it is unavailable.
fn place(app: &AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    if place_on_cursor_screen(window) {
        return;
    }
    position_on_cursor_monitor(app, window);
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(HUD_LABEL) {
        let _ = window.hide();
    }
}

/// Place the pill in Cocoa screen coordinates: points, origin at the bottom
/// left of the primary display, one coherent space across every display.
///
/// Tauri's `PhysicalPosition` is not that. Each monitor's physical rect is
/// derived from its *own* scale factor, so on a mixed-DPI setup — a Retina
/// laptop plus a 1x external, say — the physical rects neither tile nor agree
/// with the physical cursor position. `monitor_from_point` then finds nothing,
/// placement silently falls back to the primary display, and a position
/// computed for one display can land the window off every screen: exactly the
/// "recording indicator disappeared once I connected a second monitor" report.
/// Cocoa points have none of that ambiguity, so this is the primary path.
///
/// Returns false if AppKit could not be consulted, leaving the caller to fall
/// back. Must run on the main thread; `show` is dispatched there via `on_ui`.
#[cfg(target_os = "macos")]
fn place_on_cursor_screen(window: &tauri::WebviewWindow) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    // NSPoint/NSRect rather than core-graphics' equivalents: the layout is the
    // same, but only these implement objc2's `Encode`, required to send them
    // through a message.
    use objc2_foundation::{NSPoint, NSRect};

    let Ok(ptr) = window.ns_window() else {
        return false;
    };
    let ns = ptr as *mut AnyObject;

    unsafe {
        let cursor: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
        if screens.is_null() {
            return false;
        }
        let count: usize = msg_send![&*screens, count];

        let mut hit: Option<(NSRect, NSRect)> = None;
        for i in 0..count {
            let screen: *mut AnyObject = msg_send![&*screens, objectAtIndex: i];
            if screen.is_null() {
                continue;
            }
            let frame: NSRect = msg_send![&*screen, frame];
            if cursor.x >= frame.origin.x
                && cursor.x < frame.origin.x + frame.size.width
                && cursor.y >= frame.origin.y
                && cursor.y < frame.origin.y + frame.size.height
            {
                // visibleFrame excludes the Dock and menu bar.
                let visible: NSRect = msg_send![&*screen, visibleFrame];
                hit = Some((frame, visible));
                break;
            }
        }
        let Some((frame, visible)) = hit else {
            tracing::warn!(
                "no NSScreen contains the pointer at {:.0},{:.0} ({count} screens)",
                cursor.x,
                cursor.y
            );
            return false;
        };

        let x = frame.origin.x + (frame.size.width - WIDTH) / 2.0;
        // Cocoa y grows upward, so this is a margin above the screen's bottom
        // edge, lifted clear of the Dock when the Dock is docked there.
        let y = (frame.origin.y + BOTTOM_MARGIN).max(visible.origin.y + DOCK_GAP);
        let _: () = msg_send![&mut *ns, setFrameOrigin: NSPoint::new(x, y)];
        tracing::info!(
            "pill → screen {:.0},{:.0} {:.0}x{:.0}, placed at {:.0},{:.0} (cursor {:.0},{:.0})",
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            x,
            y,
            cursor.x,
            cursor.y
        );
        true
    }
}

/// The monitor the pill should appear on: the one under the pointer, falling
/// back to the primary. `monitor_from_point` returns nothing on some
/// multi-display layouts, so the cursor is also matched against the monitor
/// list by hand before giving up — otherwise the pill lands on the primary
/// display while the user is looking at the other one.
fn target_monitor(app: &AppHandle) -> Option<tauri::Monitor> {
    let cursor = app.cursor_position().ok();
    if let Some(pos) = cursor {
        if let Ok(Some(monitor)) = app.monitor_from_point(pos.x, pos.y) {
            return Some(monitor);
        }
        if let Ok(monitors) = app.available_monitors() {
            let hit = monitors.into_iter().find(|m| {
                let (o, size) = (m.position(), m.size());
                let (x0, y0) = (o.x as f64, o.y as f64);
                pos.x >= x0
                    && pos.y >= y0
                    && pos.x < x0 + size.width as f64
                    && pos.y < y0 + size.height as f64
            });
            if hit.is_some() {
                return hit;
            }
        }
        tracing::warn!("no monitor contains the cursor at {:.0},{:.0}", pos.x, pos.y);
    }
    app.primary_monitor().ok().flatten()
}

fn position_on_cursor_monitor(app: &AppHandle, window: &tauri::WebviewWindow) {
    let Some(monitor) = target_monitor(app) else {
        tracing::warn!("no monitor available to place the recording pill on");
        return;
    };

    let scale = monitor.scale_factor();
    let mpos = monitor.position();
    let msize = monitor.size();
    let w = WIDTH * scale;
    let h = HEIGHT * scale;
    let x = mpos.x as f64 + (msize.width as f64 - w) / 2.0;
    let y = mpos.y as f64 + msize.height as f64 - h - BOTTOM_MARGIN * scale;
    tracing::info!(
        "pill → monitor at {},{} {}x{} @{}x, placed at {:.0},{:.0}",
        mpos.x,
        mpos.y,
        msize.width,
        msize.height,
        scale,
        x,
        y
    );
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
