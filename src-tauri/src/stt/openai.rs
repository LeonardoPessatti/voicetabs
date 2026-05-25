//! OpenAI `gpt-4o-mini-transcribe` backend.
//!
//! Multipart POST to `/v1/audio/transcriptions`. One retry on 429 with a
//! ~750 ms backoff; auth and server errors map to typed `BackendError`
//! variants. The API key is held in `SecretString` so the `Debug` impl
//! never logs it, and every `to_string()` we forward through `redact_secret`
//! before constructing an error variant.

use std::io::Cursor;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use crate::stt::keyring::redact_secret;
use crate::stt::protocol::TranscriptionResult;

const PROD_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";
const MODEL_ID: &str = "gpt-4o-mini-transcribe";
const RETRY_BASE_MS: u64 = 500;
const REQUEST_TIMEOUT_S: u64 = 30;

pub struct OpenAiSttBackend {
    api_key: SecretString,
    endpoint: String,
    client: reqwest::Client,
}

impl OpenAiSttBackend {
    pub fn new(api_key: String) -> Self {
        Self::new_with_endpoint(api_key, PROD_ENDPOINT.into())
    }

    pub fn new_with_endpoint(api_key: String, endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_S))
            .build()
            .expect("build reqwest client");
        Self {
            api_key: SecretString::from(api_key),
            endpoint,
            client,
        }
    }

    fn build_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::<u8>::new());
        {
            let mut w =
                hound::WavWriter::new(&mut cursor, spec).expect("build WavWriter");
            for &s in samples {
                let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                w.write_sample(v).expect("write sample");
            }
            w.finalize().expect("finalize wav");
        }
        cursor.into_inner()
    }

    fn scrub(&self, s: String) -> String {
        redact_secret(&s, self.api_key.expose_secret())
    }

    async fn post_once(
        &self,
        req: &TranscribeRequest,
        wav: &[u8],
    ) -> Result<reqwest::Response, BackendError> {
        let part = Part::bytes(wav.to_vec())
            .file_name("utterance.wav")
            .mime_str("audio/wav")
            .map_err(|e| BackendError::Network(self.scrub(e.to_string())))?;
        let form = Form::new()
            .text("model", MODEL_ID.to_string())
            .text("language", req.language.clone())
            .text("prompt", req.initial_prompt.clone())
            .text("response_format", "json".to_string())
            .part("file", part);
        let resp = self
            .client
            .post(&self.endpoint)
            .bearer_auth(self.api_key.expose_secret())
            .multipart(form)
            .send()
            .await
            .map_err(|e| BackendError::Network(self.scrub(e.to_string())))?;
        Ok(resp)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}

#[async_trait]
impl SttBackend for OpenAiSttBackend {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError> {
        let started = std::time::Instant::now();
        let wav = Self::build_wav(&req.samples, req.sample_rate);
        let mut attempt: u32 = 0;
        let resp = loop {
            let r = self.post_once(&req, &wav).await?;
            match r.status() {
                s if s.is_success() => break r,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    return Err(BackendError::Auth);
                }
                StatusCode::TOO_MANY_REQUESTS if attempt == 0 => {
                    attempt += 1;
                    let backoff = Duration::from_millis(RETRY_BASE_MS + 250);
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    return Err(BackendError::RateLimited);
                }
                s if s.is_server_error() => {
                    let body = r.text().await.unwrap_or_default();
                    return Err(BackendError::Server(
                        self.scrub(format!("{s}: {body}")),
                    ));
                }
                s => {
                    let body = r.text().await.unwrap_or_default();
                    return Err(BackendError::Server(
                        self.scrub(format!("unexpected {s}: {body}")),
                    ));
                }
            }
        };
        let body: OpenAiTranscriptionResponse = resp
            .json()
            .await
            .map_err(|e| BackendError::Server(self.scrub(e.to_string())))?;
        Ok(TranscriptionResult {
            request_id: req.request_id,
            text: body.text,
            avg_logprob: 0.0,
            no_speech_prob: 0.0,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
    fn backend_id(&self) -> &str {
        "openai"
    }
    fn model_id(&self) -> &str {
        MODEL_ID
    }
}
