//! IPC JSON message types. Matches `docs/superpowers/specs/2026-05-20-voicetabs-design.md` §7.4.

use serde::{Deserialize, Serialize};

/// Sent by the worker once on startup, after the model loads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyMessage {
    pub ready: bool,
    pub model_id: String,
    pub backend: String, // "cpu" only (Phase 7 cloud path doesn't use this subprocess)
}

/// Sent by the main process before each PCM payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHeader {
    pub request_id: String,
    pub sample_rate: u32,
    pub language: String,
    pub initial_prompt: String,
    pub n_samples: u32,
}

/// Sent by the worker after running inference. Exactly one of `text` or
/// `error` is populated. The fields are flat (not nested) to match the spec
/// example payloads.
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

impl ResponseMessage {
    pub fn ok(
        request_id: String,
        text: String,
        avg_logprob: f32,
        no_speech_prob: f32,
        duration_ms: u64,
    ) -> Self {
        Self {
            request_id,
            text: Some(text),
            avg_logprob: Some(avg_logprob),
            no_speech_prob: Some(no_speech_prob),
            duration_ms: Some(duration_ms),
            error: None,
        }
    }

    pub fn err(request_id: String, error: String) -> Self {
        Self {
            request_id,
            text: None,
            avg_logprob: None,
            no_speech_prob: None,
            duration_ms: None,
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_message_round_trip() {
        let m = ReadyMessage {
            ready: true,
            model_id: "ggml-large-v3-turbo-q5_0".into(),
            backend: "cpu".into(),
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: ReadyMessage = serde_json::from_str(&s).unwrap();
        assert!(back.ready);
        assert_eq!(back.backend, "cpu");
    }

    #[test]
    fn request_header_round_trip() {
        let h = RequestHeader {
            request_id: "abc".into(),
            sample_rate: 16_000,
            language: "pt".into(),
            initial_prompt: "VoiceTabs".into(),
            n_samples: 32_000,
        };
        let s = serde_json::to_string(&h).unwrap();
        let back: RequestHeader = serde_json::from_str(&s).unwrap();
        assert_eq!(back.n_samples, 32_000);
    }

    #[test]
    fn response_ok_serializes_without_error_field() {
        let r = ResponseMessage::ok("req-1".into(), "hello".into(), -0.3, 0.02, 450);
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("\"error\""));
        assert!(s.contains("\"text\""));
    }

    #[test]
    fn response_err_serializes_without_text_field() {
        let r = ResponseMessage::err("req-2".into(), "boom".into());
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"error\""));
        assert!(!s.contains("\"text\""));
    }
}
