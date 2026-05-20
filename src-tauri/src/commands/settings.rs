use tauri::State;

use crate::db::{settings as repo, Db};

use super::tabs::CommandError;

impl From<repo::SettingsError> for CommandError {
    fn from(e: repo::SettingsError) -> Self {
        CommandError { code: "SETTINGS_ERROR".into(), message: e.to_string() }
    }
}

#[tauri::command]
pub fn settings_get(key: String, db: State<'_, Db>) -> Result<Option<String>, CommandError> {
    repo::get_raw(&db, &key).map_err(Into::into)
}

#[tauri::command]
pub fn settings_set(key: String, value: String, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::set_raw(&db, &key, &value).map_err(Into::into)
}
