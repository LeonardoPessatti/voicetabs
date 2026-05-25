//! Hot-swappable active backend handle.
//!
//! Wraps an `Arc<dyn SttBackend>` in a Tokio `RwLock`. Callers `snapshot()`
//! to grab the current backend (brief read lock, dropped before `await`),
//! then call `transcribe()` against the snapshot — so an in-flight call
//! does not block a subsequent `replace()` and vice versa. The snapshot
//! caller sees the backend that was active at snapshot time, even if a
//! swap happens mid-call.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock as TokioRwLock;

use crate::stt::backend::SttBackend;

#[derive(Clone)]
pub struct ActiveBackend {
    inner: Arc<TokioRwLock<Arc<dyn SttBackend>>>,
}

impl ActiveBackend {
    pub fn new(initial: Arc<dyn SttBackend>) -> Self {
        Self {
            inner: Arc::new(TokioRwLock::new(initial)),
        }
    }

    /// Snapshot the current backend Arc. Short read lock; caller drops it
    /// before awaiting on the returned backend's `transcribe`.
    pub async fn snapshot(&self) -> Arc<dyn SttBackend> {
        self.inner.read().await.clone()
    }

    /// Replace the active backend. Brief write lock.
    pub async fn replace(&self, new: Arc<dyn SttBackend>) {
        *self.inner.write().await = new;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Local,
    Openai,
}

impl BackendKind {
    pub fn from_setting(s: &str) -> Self {
        match s {
            "openai" => BackendKind::Openai,
            _ => BackendKind::Local,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendKind::Local => "local",
            BackendKind::Openai => "openai",
        }
    }
}

/// Build a fresh `Arc<dyn SttBackend>` for `kind`. For `Openai` this reads
/// the API key from the OS keyring; missing key → `Err(BackendError::MissingApiKey)`.
pub fn build_backend(
    kind: BackendKind,
    local: Arc<dyn SttBackend>,
) -> Result<Arc<dyn SttBackend>, crate::stt::backend::BackendError> {
    use crate::stt::backend::BackendError;
    use crate::stt::keyring as kr;
    use crate::stt::openai::OpenAiSttBackend;
    match kind {
        BackendKind::Local => Ok(local),
        BackendKind::Openai => {
            let key = kr::get_api_key().map_err(|_| BackendError::MissingApiKey)?;
            Ok(Arc::new(OpenAiSttBackend::new(key)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::backend::{BackendError, SttBackend, TranscribeRequest};
    use crate::stt::protocol::TranscriptionResult;
    use async_trait::async_trait;

    struct Tagged(&'static str);
    #[async_trait]
    impl SttBackend for Tagged {
        async fn transcribe(
            &self,
            req: TranscribeRequest,
        ) -> Result<TranscriptionResult, BackendError> {
            Ok(TranscriptionResult {
                request_id: req.request_id,
                text: self.0.into(),
                avg_logprob: 0.0,
                no_speech_prob: 0.0,
                duration_ms: 1,
            })
        }
        fn backend_id(&self) -> &str {
            self.0
        }
        fn model_id(&self) -> &str {
            "m"
        }
    }

    fn req(id: &str) -> TranscribeRequest {
        TranscribeRequest {
            request_id: id.into(),
            samples: vec![],
            sample_rate: 16_000,
            language: "pt".into(),
            initial_prompt: String::new(),
            audio_path: None,
            started_at_ms: 0,
        }
    }

    #[tokio::test]
    async fn replace_swaps_for_next_call() {
        let h = ActiveBackend::new(Arc::new(Tagged("a")));
        let snap = h.snapshot().await;
        let r1 = snap.transcribe(req("1")).await.unwrap();
        assert_eq!(r1.text, "a");

        h.replace(Arc::new(Tagged("b"))).await;
        let snap2 = h.snapshot().await;
        let r2 = snap2.transcribe(req("2")).await.unwrap();
        assert_eq!(r2.text, "b");

        // The pre-swap snapshot still points at the old backend.
        let r3 = snap.transcribe(req("3")).await.unwrap();
        assert_eq!(r3.text, "a", "old snapshot must not see new backend");
    }

    #[test]
    fn kind_parses_known_values() {
        assert_eq!(BackendKind::from_setting("local"), BackendKind::Local);
        assert_eq!(BackendKind::from_setting("openai"), BackendKind::Openai);
        assert_eq!(BackendKind::from_setting("garbage"), BackendKind::Local);
    }
}
