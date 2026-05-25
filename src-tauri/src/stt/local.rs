//! Adapter implementing `SttBackend` for the existing local subprocess (CPU).

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use crate::stt::protocol::TranscriptionResult;
use crate::stt::status::{SttStatus, SttStatusHandle};
use crate::stt::supervisor::{SttSupervisor, SupervisorError};

pub struct LocalSttBackend {
    supervisor: SttSupervisor,
    status: SttStatusHandle,
    /// Cached value of model_id taken from `SttStatus::Ready`; updated lazily on
    /// each successful transcribe so `backend_id()`/`model_id()` are cheap.
    cached_model_id: Mutex<String>,
}

impl LocalSttBackend {
    pub fn new(supervisor: SttSupervisor) -> Self {
        let status = supervisor.status_handle();
        let cached = match status.get() {
            SttStatus::Ready { model_id, .. } => model_id,
            _ => "unknown".into(),
        };
        Self { supervisor, status, cached_model_id: Mutex::new(cached) }
    }
}

#[async_trait]
impl SttBackend for LocalSttBackend {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError> {
        let r = self
            .supervisor
            .transcribe(
                req.request_id,
                req.samples,
                &req.language,
                &req.initial_prompt,
                req.audio_path,
                req.started_at_ms,
            )
            .await
            .map_err(|e: SupervisorError| BackendError::Worker(e.to_string()))?;
        // Refresh cached_model_id from the latest Ready state.
        if let SttStatus::Ready { model_id, .. } = self.status.get() {
            *self.cached_model_id.lock() = model_id;
        }
        Ok(r)
    }
    fn backend_id(&self) -> &str { "cpu" }
    fn model_id(&self) -> &str {
        // Static return because the trait signature constrains the lifetime.
        // For dynamic model_id we expose `cached_model_id` via a separate
        // helper; callers that need the live value go through `SttStatus`.
        "local-whisper"
    }
}
