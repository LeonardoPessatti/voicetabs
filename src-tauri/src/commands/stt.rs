use tauri::State;

use crate::stt::{SttStatus, SttStatusHandle};

use super::tabs::CommandError;

#[tauri::command]
pub fn stt_status(status: State<'_, SttStatusHandle>) -> Result<SttStatus, CommandError> {
    Ok(status.get())
}
