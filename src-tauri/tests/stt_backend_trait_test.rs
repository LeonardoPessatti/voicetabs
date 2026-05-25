//! Sanity check that the trait is object-safe (we use `dyn SttBackend`) and that
//! a stub implementation routes one request to one response.

use std::sync::Arc;
use voicetabs_lib::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use voicetabs_lib::stt::protocol::TranscriptionResult;

struct StubBackend;

#[async_trait::async_trait]
impl SttBackend for StubBackend {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError> {
        Ok(TranscriptionResult {
            request_id: req.request_id,
            text: format!("stub-{}", req.samples.len()),
            avg_logprob: 0.0,
            no_speech_prob: 0.0,
            duration_ms: 10,
        })
    }
    fn backend_id(&self) -> &str { "stub" }
    fn model_id(&self) -> &str { "stub-model" }
}

#[tokio::test]
async fn trait_is_object_safe_and_routes_one_call() {
    let b: Arc<dyn SttBackend> = Arc::new(StubBackend);
    let req = TranscribeRequest {
        request_id: "r1".into(),
        samples: vec![0.0_f32; 16_000],
        sample_rate: 16_000,
        language: "pt".into(),
        initial_prompt: String::new(),
        audio_path: None,
        started_at_ms: 0,
    };
    let r = b.transcribe(req).await.unwrap();
    assert_eq!(r.text, "stub-16000");
    assert_eq!(b.backend_id(), "stub");
    assert_eq!(b.model_id(), "stub-model");
}
