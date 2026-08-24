//! Tray popover: a small always-on-top quick panel toggled by left-clicking
//! the tray icon, positioned under the icon and hidden when it loses focus
//! (the blur handler lives in lib.rs's on_window_event).

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

pub const POPOVER_LABEL: &str = "popover";
const WIDTH: f64 = 340.0;
const HEIGHT: f64 = 420.0;
/// Distance from the click point (mid menu bar) down to the panel top; clears
/// both the standard 24 pt bar and the 37 pt notch bar.
const BELOW_CLICK: f64 = 24.0;
const EDGE_MARGIN: f64 = 8.0;

pub fn ensure(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(POPOVER_LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, POPOVER_LABEL, WebviewUrl::App("popover.html".into()))
        .title("Speakly")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    Ok(())
}

/// Left-click on the tray icon: hide if showing, otherwise place the panel
/// under the clicked icon and show it focused (focus is what makes
/// click-outside dismiss it via the blur event).
pub fn toggle(app: &AppHandle, click: PhysicalPosition<f64>) {
    let Some(window) = app.get_webview_window(POPOVER_LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }

    let monitor = app
        .monitor_from_point(click.x, click.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let mpos = monitor.position();
        let msize = monitor.size();
        let w = WIDTH * scale;
        let margin = EDGE_MARGIN * scale;

        let min_x = mpos.x as f64 + margin;
        let max_x = mpos.x as f64 + msize.width as f64 - w - margin;
        let x = (click.x - w / 2.0).clamp(min_x, max_x.max(min_x));
        let y = click.y + BELOW_CLICK * scale;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.show();
    let _ = window.set_focus();
}
