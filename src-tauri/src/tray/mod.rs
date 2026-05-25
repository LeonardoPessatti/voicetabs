//! System tray icon + menu.
//!
//! Built once in `lib.rs::run()::setup()`. Owns:
//! - The tray icon (color reflects capture state).
//! - The menu (Show/Hide, Capture toggle, Mode toggle, Quit).
//! - The `should_exit` flag — when Quit is clicked we set it before
//!   `app.exit(0)`; the main window's `on_window_event` checks this flag and
//!   only `prevent_close()` if the flag is false (close -> hide to tray).

pub mod menu;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::image::Image;
use tauri::tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub use menu::MenuIds;

#[derive(Clone)]
pub struct TrayHandle {
    icon: TrayIcon,
    should_exit: Arc<AtomicBool>,
}

impl TrayHandle {
    pub fn build(app: &AppHandle) -> tauri::Result<Self> {
        let should_exit = Arc::new(AtomicBool::new(false));
        let menu = menu::build_menu(app)?;
        let icon = TrayIconBuilder::with_id("voicetabs-tray")
            .icon(load_idle_icon()?)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(menu::menu_event_handler(should_exit.clone()))
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    ..
                } = event
                {
                    toggle_main_window(tray.app_handle());
                }
            })
            .build(app)?;
        Ok(Self { icon, should_exit })
    }

    pub fn set_capturing(&self, capturing: bool) -> tauri::Result<()> {
        let img = if capturing {
            load_capturing_icon()?
        } else {
            load_idle_icon()?
        };
        self.icon.set_icon(Some(img))
    }

    pub fn should_exit(&self) -> Arc<AtomicBool> {
        self.should_exit.clone()
    }
}

fn load_idle_icon() -> tauri::Result<Image<'static>> {
    let bytes: &'static [u8] = include_bytes!("../../icons/tray-idle.png");
    Image::from_bytes(bytes)
}

fn load_capturing_icon() -> tauri::Result<Image<'static>> {
    let bytes: &'static [u8] = include_bytes!("../../icons/tray-capturing.png");
    Image::from_bytes(bytes)
}

pub fn toggle_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}
