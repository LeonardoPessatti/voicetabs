use serde::Serialize;
use tauri::State;

use crate::db::{tabs as repo, Db};
use crate::routing::ActiveTab;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<repo::TabsError> for CommandError {
    fn from(e: repo::TabsError) -> Self {
        match e {
            repo::TabsError::NotFound(id) => CommandError {
                code: "TAB_NOT_FOUND".into(),
                message: format!("tab {id} not found"),
            },
            repo::TabsError::Sql(e) => CommandError {
                code: "SQL_ERROR".into(),
                message: e.to_string(),
            },
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn tabs_list(db: State<'_, Db>) -> Result<Vec<repo::Tab>, CommandError> {
    repo::list(&db).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_create(title: String, db: State<'_, Db>) -> Result<repo::Tab, CommandError> {
    repo::create(&db, &title, now_ms()).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_rename(id: i64, title: String, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::rename(&db, id, &title, now_ms()).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_delete(id: i64, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::delete(&db, id).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_reorder(ordered_ids: Vec<i64>, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::reorder(&db, &ordered_ids, now_ms()).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_set_active(
    id: i64,
    db: State<'_, Db>,
    active: State<'_, ActiveTab>,
) -> Result<(), CommandError> {
    // Verify the tab exists. If not, we still don't surface an error to the
    // user (the frontend may race with delete); just don't update the atomic.
    let exists = db
        .with(|c| {
            c.query_row::<i64, _, _>(
                "SELECT COUNT(1) FROM tabs WHERE id = ?",
                [id],
                |r| r.get(0),
            )
        })
        .map_err(|e| CommandError {
            code: "SQL_ERROR".into(),
            message: e.to_string(),
        })?;
    if exists == 0 {
        // No-op; the frontend will re-issue `setActive` after refreshing.
        tracing::debug!("tabs_set_active: tab {id} not found, ignoring");
        return Ok(());
    }
    active.set(id);
    Ok(())
}
