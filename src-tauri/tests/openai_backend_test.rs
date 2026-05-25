//! Tests the OpenAI backend against a mock HTTP server. Covers happy path,
//! 401 (auth), 429 (rate-limited with retry), and 500 (server error).

use voicetabs_lib::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use voicetabs_lib::stt::openai::OpenAiSttBackend;

fn sample_request() -> TranscribeRequest {
    TranscribeRequest {
        request_id: "r1".into(),
        // 0.25 s of zeros at 16 kHz — small enough for fast multipart build,
        // large enough for the WAV writer to produce a valid header.
        samples: vec![0.0_f32; 4_000],
        sample_rate: 16_000,
        language: "pt".into(),
        initial_prompt: String::new(),
        audio_path: None,
        started_at_ms: 0,
    }
}

#[tokio::test]
async fn happy_path_returns_text() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"text":"olá mundo"}"#)
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let r = backend.transcribe(sample_request()).await.unwrap();
    assert_eq!(r.text, "olá mundo");
    assert_eq!(r.avg_logprob, 0.0);
    assert_eq!(r.no_speech_prob, 0.0);
    // duration_ms is monotonic elapsed; on a fast machine it can be 0.
    // Just assert the call returned without panicking.
    let _ = r.duration_ms;
}

#[tokio::test]
async fn auth_failure_maps_to_auth_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(401)
        .with_body(r#"{"error":{"message":"invalid api key"}}"#)
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    assert!(matches!(err, BackendError::Auth));
}

#[tokio::test]
async fn rate_limit_retries_once_then_fails() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(429)
        .with_body("rate limited")
        .expect(2) // initial + 1 retry
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    assert!(matches!(err, BackendError::RateLimited));
    m.assert_async().await;
}

#[tokio::test]
async fn rate_limit_then_success() {
    let mut server = mockito::Server::new_async().await;
    let _m_429 = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(429)
        .with_body("rate limited")
        .expect(1)
        .create_async()
        .await;
    let _m_ok = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(200)
        .with_body(r#"{"text":"recovered"}"#)
        .expect(1)
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let r = backend.transcribe(sample_request()).await.unwrap();
    assert_eq!(r.text, "recovered");
}

#[tokio::test]
async fn server_error_maps_to_server_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(500)
        .with_body("oops")
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    assert!(matches!(err, BackendError::Server(_)));
}

#[tokio::test]
async fn error_strings_never_contain_api_key() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(401)
        .with_body("body that does not contain the key")
        .create_async()
        .await;

    let key = "sk-SECRETSECRET";
    let backend = OpenAiSttBackend::new_with_endpoint(
        key.into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    let s = format!("{err:?} / {err}");
    assert!(!s.contains("SECRETSECRET"), "leaked key: {s}");
}
