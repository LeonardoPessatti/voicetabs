//! Tauri commands for backend selection + OpenAI API key management.
//!
//! All business logic lives in `stt::active` and `stt::keyring`; these are
//! thin wrappers that translate `BackendError`/`KeyringError` into typed
//! `CommandError`s for the frontend.

use std::sync::Arc;

use tauri::State;

use crate::db::{settings as settings_repo, Db};
use crate::stt::active::{build_backend, ActiveBackend, BackendKind};
use crate::stt::backend::{BackendError, SttBackend};
use crate::stt::keyring as kr;
use crate::stt::local::LocalSttBackend;
use crate::stt::status::{SttStatus, SttStatusHandle};

use super::tabs::CommandError;

const SETTING_BACKEND: &str = "stt_backend";
const SETTING_KEY_SET: &str = "openai_api_key_set";

fn cmd_err(code: &str, msg: impl Into<String>) -> CommandError {
    CommandError {
        code: code.into(),
        message: msg.into(),
    }
}

#[tauri::command]
pub fn backend_get(db: State<'_, Db>) -> Result<String, CommandError> {
    let s = settings_repo::get::<String>(&db, SETTING_BACKEND)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?
        .unwrap_or_else(|| "local".into());
    Ok(s)
}

#[tauri::command]
pub async fn backend_set(
    kind: String,
    db: State<'_, Db>,
    active: State<'_, ActiveBackend>,
    local_backend: State<'_, Arc<LocalSttBackend>>,
    status: State<'_, SttStatusHandle>,
) -> Result<(), CommandError> {
    let db = db.inner().clone();
    let active = active.inner().clone();
    let local: Arc<dyn SttBackend> = local_backend.inner().clone();
    let status = status.inner().clone();

    let bk = BackendKind::from_setting(&kind);
    let new = build_backend(bk, local).map_err(|e: BackendError| match e {
        BackendError::MissingApiKey => cmd_err(
            "OPENAI_NO_KEY",
            "configure an OpenAI API key before selecting this backend",
        ),
        other => cmd_err("BACKEND_BUILD_ERROR", other.to_string()),
    })?;
    active.replace(new).await;
    settings_repo::set(&db, SETTING_BACKEND, &bk.as_str().to_string())
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;
    let model_id = match bk {
        BackendKind::Local => match status.get() {
            SttStatus::Ready { model_id, .. } => model_id,
            _ => "local-whisper".into(),
        },
        BackendKind::Openai => "gpt-4o-mini-transcribe".into(),
    };
    status.set(SttStatus::Ready {
        backend: bk.as_str().into(),
        model_id,
    });
    Ok(())
}

#[tauri::command]
pub fn openai_key_set(value: String, db: State<'_, Db>) -> Result<(), CommandError> {
    if value.trim().is_empty() {
        return Err(cmd_err("EMPTY_KEY", "API key must not be empty"));
    }
    kr::set_api_key(&value).map_err(|e| cmd_err("KEYRING_ERROR", e.to_string()))?;
    settings_repo::set(&db, SETTING_KEY_SET, &true)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn openai_key_clear(
    db: State<'_, Db>,
    active: State<'_, ActiveBackend>,
    local_backend: State<'_, Arc<LocalSttBackend>>,
    status: State<'_, SttStatusHandle>,
) -> Result<(), CommandError> {
    let db = db.inner().clone();
    let active = active.inner().clone();
    let local: Arc<dyn SttBackend> = local_backend.inner().clone();
    let status = status.inner().clone();

    kr::clear_api_key().map_err(|e| cmd_err("KEYRING_ERROR", e.to_string()))?;
    settings_repo::set(&db, SETTING_KEY_SET, &false)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;

    // If OpenAI was the active backend, fall back to Local automatically.
    let current_setting = settings_repo::get::<String>(&db, SETTING_BACKEND)
        .ok()
        .flatten()
        .unwrap_or_else(|| "local".into());
    if current_setting == "openai" {
        settings_repo::set(&db, SETTING_BACKEND, &"local".to_string())
            .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;
        active.replace(local).await;
        let model_id = match status.get() {
            SttStatus::Ready { model_id, .. } => model_id,
            _ => "local-whisper".into(),
        };
        status.set(SttStatus::Ready {
            backend: "cpu".into(),
            model_id,
        });
    }
    Ok(())
}

#[tauri::command]
pub fn openai_key_status(db: State<'_, Db>) -> Result<bool, CommandError> {
    let v: bool = settings_repo::get(&db, SETTING_KEY_SET)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?
        .unwrap_or(false);
    Ok(v)
}
