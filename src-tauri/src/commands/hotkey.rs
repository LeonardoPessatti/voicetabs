use std::time::Duration;

use tauri::State;

use crate::db::Db;
use crate::hotkey::{Binding, HotkeyManager};

use super::tabs::CommandError;

#[tauri::command]
pub fn hotkey_get_binding(
    mgr: State<'_, HotkeyManager>,
) -> Result<Option<Binding>, CommandError> {
    Ok(mgr.current_binding())
}

#[tauri::command]
pub fn hotkey_set_binding(
    mgr: State<'_, HotkeyManager>,
    db: State<'_, Db>,
    binding: Binding,
) -> Result<(), CommandError> {
    mgr.set_binding(binding.clone()).map_err(|e| CommandError {
        code: "HOTKEY_ERROR".into(),
        message: e.to_string(),
    })?;
    crate::db::settings::set(&db, "hotkey_binding", &binding).map_err(|e| CommandError {
        code: "SETTINGS_ERROR".into(),
        message: e.to_string(),
    })?;
    Ok(())
}

#[tauri::command]
pub fn hotkey_clear_binding(
    mgr: State<'_, HotkeyManager>,
    db: State<'_, Db>,
) -> Result<(), CommandError> {
    mgr.clear_binding();
    let _ = crate::db::settings::delete(&db, "hotkey_binding");
    Ok(())
}

#[tauri::command]
pub fn hotkey_capture_next(
    mgr: State<'_, HotkeyManager>,
) -> Result<Binding, CommandError> {
    mgr.capture_next_press(Duration::from_secs(15))
        .map_err(|e| CommandError {
            code: "VALIDATION".into(),
            message: e.to_string(),
        })
}
