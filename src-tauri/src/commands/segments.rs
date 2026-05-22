use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{segments as repo, settings as settings_repo, Db};
use crate::paths;
use crate::stt::status::{SttStatus, SttStatusHandle};
use crate::stt::SttSupervisor;
use crate::vocab::build_initial_prompt;

use super::tabs::CommandError;

impl From<repo::SegmentsError> for CommandError {
    fn from(e: repo::SegmentsError) -> Self {
        match e {
            repo::SegmentsError::NotFound(id) => CommandError {
                code: "SEGMENT_NOT_FOUND".into(),
                message: format!("segment {id} not found"),
            },
            repo::SegmentsError::TabNotFound(id) => CommandError {
                code: "TAB_NOT_FOUND".into(),
                message: format!("tab {id} not found"),
            },
            repo::SegmentsError::Sql(e) => CommandError {
                code: "SQL_ERROR".into(),
                message: e.to_string(),
            },
        }
    }
}

#[tauri::command]
pub fn segments_list_for_tab(
    tab_id: i64,
    db: State<'_, Db>,
) -> Result<Vec<repo::Segment>, CommandError> {
    repo::list_for_tab(&db, tab_id).map_err(Into::into)
}

#[tauri::command]
pub fn segments_update(
    id: i64,
    text: String,
    db: State<'_, Db>,
) -> Result<(), CommandError> {
    repo::update_text(&db, id, &text).map_err(Into::into)
}

#[tauri::command]
pub fn segments_delete(
    id: i64,
    db: State<'_, Db>,
) -> Result<(), CommandError> {
    // Look up the audio path BEFORE deleting the row so we can delete the
    // WAV. If the file is already gone we don't fail the command — log and
    // continue. The row is gone, which is the user's intent.
    let segment = repo::get_by_id(&db, id)?;
    repo::delete(&db, id)?;

    let audio_dir = paths::audio_dir().map_err(|e| CommandError {
        code: "PATHS_ERROR".into(),
        message: e.to_string(),
    })?;
    let full = audio_dir.join(&segment.audio_path);
    match std::fs::remove_file(&full) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!("audio file already absent: {full:?}");
        }
        Err(e) => {
            tracing::warn!("failed to delete audio file {full:?}: {e}");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetranscribeMode {
    /// Use today's vocab from the `vocab_terms` setting.
    Current,
    /// Use the vocab stored on this segment row at original-transcription time.
    Snapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetranscribeResult {
    pub text: String,
    pub model_id: String,
    pub avg_logprob: f64,
    pub no_speech_prob: f64,
}

#[tauri::command]
pub async fn segments_retranscribe(
    id: i64,
    mode: RetranscribeMode,
    db: State<'_, Db>,
    stt: State<'_, SttSupervisor>,
    stt_status: State<'_, SttStatusHandle>,
) -> Result<RetranscribeResult, CommandError> {
    // Clone every State value up front. We do not hold any `State<'_, …>`
    // borrow across `.await`, which keeps the future `Send` and avoids
    // lifetime issues with Tauri's borrow-checking on async commands.
    let db = db.inner().clone();
    let supervisor = stt.inner().clone();
    let stt_status = stt_status.inner().clone();

    let segment = repo::get_by_id(&db, id)?;

    // 1. Read audio WAV → f32 samples.
    let audio_dir = paths::audio_dir().map_err(|e| CommandError {
        code: "PATHS_ERROR".into(),
        message: e.to_string(),
    })?;
    let full = audio_dir.join(&segment.audio_path);
    let samples = read_wav_to_f32(&full).map_err(|e| CommandError {
        code: "WAV_READ_ERROR".into(),
        message: e.to_string(),
    })?;

    // 2. Pick vocab.
    let vocab_terms = match mode {
        RetranscribeMode::Current => settings_repo::get::<Vec<String>>(&db, "vocab_terms")
            .map_err(|e| CommandError {
                code: "SETTINGS_ERROR".into(),
                message: e.to_string(),
            })?
            .unwrap_or_default(),
        RetranscribeMode::Snapshot => {
            serde_json::from_str::<Vec<String>>(&segment.vocab_snapshot).unwrap_or_default()
        }
    };

    // 3. Build initial_prompt and call STT.
    let language: String = settings_repo::get::<String>(&db, "language")
        .ok()
        .flatten()
        .unwrap_or_else(|| "pt".into());
    let initial_prompt = build_initial_prompt(&vocab_terms, &language);

    let request_id = uuid::Uuid::new_v4().to_string();
    let started_at_ms = u64::try_from(segment.started_at).unwrap_or(0);
    let result = supervisor
        .transcribe(
            request_id,
            samples,
            &language,
            &initial_prompt,
            Some(full.clone()),
            started_at_ms,
        )
        .await
        .map_err(|e| CommandError {
            code: "STT_ERROR".into(),
            message: e.to_string(),
        })?;

    // The supervisor's TranscriptionResult does not carry model_id (the worker
    // reports it once at handshake via ReadyMessage). Pull it from the
    // SttStatusHandle, which the supervisor publishes into on every (re)boot.
    let model_id = match stt_status.get() {
        SttStatus::Ready { model_id, .. } => model_id,
        // If the worker is in another state (loading/restarting/error) we
        // shouldn't have gotten a successful transcribe above, but defend
        // anyway: preserve the previous model_id stored on the segment.
        _ => segment.model_id.clone(),
    };

    // 4. Persist.
    repo::update_retranscribed(
        &db,
        id,
        &result.text,
        &model_id,
        result.avg_logprob as f64,
        result.no_speech_prob as f64,
    )?;

    Ok(RetranscribeResult {
        text: result.text,
        model_id,
        avg_logprob: result.avg_logprob as f64,
        no_speech_prob: result.no_speech_prob as f64,
    })
}

fn read_wav_to_f32(path: &PathBuf) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    // Our writer always produces 16 kHz mono i16. Defensive code: convert
    // regardless of the on-disk format so a hand-edited file or a future
    // format change does not panic.
    let samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int => reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|f| (f.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect(),
    };
    Ok(samples.into_iter().map(|s| s as f32 / 32767.0).collect())
}
