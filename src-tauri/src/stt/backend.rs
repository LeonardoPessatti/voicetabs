//! `SttBackend` — abstract trait spanning local subprocess and cloud HTTP impls.
//!
//! Hot-path callers (`run_segment_pipeline`, `segments_retranscribe`) snapshot
//! an `Arc<dyn SttBackend>` from `ActiveBackend`, drop the lock, then call
//! `transcribe()`. See `stt::active::ActiveBackend` for the swap semantics.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;

use crate::stt::protocol::TranscriptionResult;

#[derive(Debug, Clone)]
pub struct TranscribeRequest {
    pub request_id: String,
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub language: String,
    pub initial_prompt: String,
    pub audio_path: Option<PathBuf>,
    pub started_at_ms: u64,
}

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendError {
    #[error("missing OpenAI API key")]
    MissingApiKey,
    #[error("authentication failed (check API key)")]
    Auth,
    #[error("network: {0}")]
    Network(String),
    #[error("rate limited")]
    RateLimited,
    #[error("server error: {0}")]
    Server(String),
    #[error("worker error: {0}")]
    Worker(String),
    #[error("backend not ready")]
    NotReady,
}

#[async_trait]
pub trait SttBackend: Send + Sync {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError>;
    fn backend_id(&self) -> &str;
    fn model_id(&self) -> &str;
}
