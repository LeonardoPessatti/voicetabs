use tauri::State;

use crate::capture::{CaptureController, CaptureMode, CaptureModeHandle, CaptureStatus};

use super::tabs::CommandError;

#[tauri::command]
pub fn capture_start(controller: State<'_, CaptureController>) -> Result<(), CommandError> {
    controller.start();
    Ok(())
}

#[tauri::command]
pub fn capture_stop(controller: State<'_, CaptureController>) -> Result<(), CommandError> {
    controller.stop();
    Ok(())
}

#[tauri::command]
pub fn capture_status(controller: State<'_, CaptureController>) -> Result<CaptureStatus, CommandError> {
    Ok(controller.status())
}

#[tauri::command]
pub fn capture_get_mode(handle: State<'_, CaptureModeHandle>) -> Result<String, CommandError> {
    Ok(handle.get().as_str().to_string())
}

#[tauri::command]
pub fn capture_set_mode(
    handle: State<'_, CaptureModeHandle>,
    db: State<'_, crate::db::Db>,
    mode: String,
) -> Result<(), CommandError> {
    let parsed = CaptureMode::parse(&mode).ok_or_else(|| CommandError {
        code: "VALIDATION".into(),
        message: format!("unknown capture_mode: {mode}"),
    })?;
    handle.set(parsed);
    crate::db::settings::set(&db, "capture_mode", &parsed.as_str().to_string()).map_err(|e| {
        CommandError {
            code: "SETTINGS_ERROR".into(),
            message: e.to_string(),
        }
    })?;
    Ok(())
}
