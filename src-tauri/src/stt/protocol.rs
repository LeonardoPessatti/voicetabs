//! IPC message types. Mirrors `stt_worker/src/protocol.rs`. See spec §7.4.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyMessage {
    pub ready: bool,
    pub model_id: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHeader {
    pub request_id: String,
    pub sample_rate: u32,
    pub language: String,
    pub initial_prompt: String,
    pub n_samples: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_logprob: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_speech_prob: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The transcription result presented to the rest of the main process. This is
/// the canonical type used by `SttClient::transcribe`, the supervisor's
/// in-flight queue, and the `stt-transcription` Tauri event payload.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionResult {
    pub request_id: String,
    pub text: String,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub duration_ms: u64,
}

impl TranscriptionResult {
    pub fn from_response(r: ResponseMessage) -> Result<Self, String> {
        match (r.text, r.error) {
            (Some(text), None) => Ok(Self {
                request_id: r.request_id,
                text,
                avg_logprob: r.avg_logprob.unwrap_or(-1.0),
                no_speech_prob: r.no_speech_prob.unwrap_or(0.0),
                duration_ms: r.duration_ms.unwrap_or(0),
            }),
            (_, Some(err)) => Err(err),
            (None, None) => Err("response missing both text and error".into()),
        }
    }
}
