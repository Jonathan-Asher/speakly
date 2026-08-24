//! Menu bar presence: tray icon with a small state indicator in its title and
//! a minimal menu. Menu-bar-first behavior (accessory activation policy) comes
//! with the popover work.

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

pub const TRAY_ID: &str = "speakly-tray";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Open Speakly").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Speakly").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().expect("app icon").clone())
        .icon_as_template(true)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// Reflect dictation state next to the icon: ● recording, … transcribing.
pub fn set_state(app: &AppHandle, state: &str) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let title = match state {
        "listening" => Some("●"),
        "transcribing" | "pasting" => Some("…"),
        _ => None,
    };
    let _ = tray.set_title(title);
}
