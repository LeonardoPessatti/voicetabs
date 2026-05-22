use tauri::State;

use crate::capture::{CaptureController, CaptureStatus};

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
