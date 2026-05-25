use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Manager};

use crate::capture::{CaptureController, CaptureMode, CaptureModeHandle, CaptureStatus};

pub struct MenuIds;
impl MenuIds {
    pub const TOGGLE_WINDOW: &'static str = "tray:toggle_window";
    pub const TOGGLE_CAPTURE: &'static str = "tray:toggle_capture";
    pub const TOGGLE_MODE: &'static str = "tray:toggle_mode";
    pub const QUIT: &'static str = "tray:quit";
}

/// Build the static tray menu. Labels are English fallbacks; in v1 the tray
/// menu is built once at startup and not re-rendered on locale change.
/// Switching language updates the in-app UI but not the tray — documented
/// limitation.
pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let toggle = MenuItem::with_id(
        app,
        MenuIds::TOGGLE_WINDOW,
        "Show / Hide",
        true,
        None::<&str>,
    )?;
    let capture = MenuItem::with_id(
        app,
        MenuIds::TOGGLE_CAPTURE,
        "Capture: off",
        true,
        None::<&str>,
    )?;
    let mode = MenuItem::with_id(
        app,
        MenuIds::TOGGLE_MODE,
        "Mode: always on",
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MenuIds::QUIT, "Quit", true, None::<&str>)?;
    Menu::with_items(app, &[&toggle, &capture, &mode, &sep, &quit])
}

pub fn menu_event_handler(
    should_exit: Arc<AtomicBool>,
) -> impl Fn(&AppHandle, MenuEvent) + Send + Sync + 'static {
    move |app, event| match event.id().as_ref() {
        x if x == MenuIds::TOGGLE_WINDOW => crate::tray::toggle_main_window(app),
        x if x == MenuIds::TOGGLE_CAPTURE => {
            if let Some(ctrl) = app.try_state::<CaptureController>() {
                let capturing = !matches!(ctrl.status(), CaptureStatus::Idle);
                if capturing {
                    ctrl.stop();
                } else {
                    ctrl.start();
                }
            }
        }
        x if x == MenuIds::TOGGLE_MODE => {
            if let Some(handle) = app.try_state::<CaptureModeHandle>() {
                let next = match handle.get() {
                    CaptureMode::AlwaysOn => CaptureMode::Ptt,
                    CaptureMode::Ptt => CaptureMode::AlwaysOn,
                };
                handle.set(next);
                if let Some(db) = app.try_state::<crate::db::Db>() {
                    let _ = crate::db::settings::set(
                        &db,
                        "capture_mode",
                        &next.as_str().to_string(),
                    );
                }
            }
        }
        x if x == MenuIds::QUIT => {
            should_exit.store(true, Ordering::SeqCst);
            app.exit(0);
        }
        _ => {}
    }
}
