//! `SttClient` — write requests to the worker's stdin, route stdout responses
//! back to the matching call.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{oneshot, Mutex as TokioMutex};

use crate::stt::framing::{read_frame_async, write_frame_async, FrameError};
use crate::stt::protocol::{
    ReadyMessage, RequestHeader, ResponseMessage, TranscriptionResult,
};

#[derive(Debug, thiserror::Error)]
pub enum SttClientError {
    #[error("worker channel closed before response")]
    Closed,
    #[error("worker returned error: {0}")]
    Worker(String),
    #[error("framing: {0}")]
    Framing(#[from] FrameError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

type ResponseTx = oneshot::Sender<Result<TranscriptionResult, SttClientError>>;

/// `SttClient` is `Clone` because all internal state is `Arc`-shared. Callers
/// can hand a clone to each utterance task.
#[derive(Clone)]
pub struct SttClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    stdin_tx: tokio::sync::mpsc::Sender<RequestEnvelope>,
    pending: Arc<Mutex<HashMap<String, ResponseTx>>>,
}

struct RequestEnvelope {
    header: RequestHeader,
    pcm: Vec<f32>,
}

impl SttClient {
    /// Construct the client and spawn the two background tasks (writer + reader).
    /// Returns once we've read the worker's `ReadyMessage`.
    pub async fn handshake<R, W>(
        mut stdout: R,
        stdin: W,
    ) -> Result<(Self, ReadyMessage), SttClientError>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        // 1. Read the ReadyMessage frame.
        let frame = read_frame_async(&mut stdout)
            .await?
            .ok_or(SttClientError::Closed)?;
        let ready: ReadyMessage = serde_json::from_slice(&frame)?;
        if !ready.ready {
            return Err(SttClientError::Worker("worker reported not ready".into()));
        }

        // 2. Spawn writer task: drains `stdin_rx`, writes header + PCM to stdin.
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<RequestEnvelope>(8);
        let stdin = Arc::new(TokioMutex::new(stdin));
        let stdin_for_writer = stdin.clone();
        tokio::spawn(async move {
            while let Some(env) = stdin_rx.recv().await {
                let mut w = stdin_for_writer.lock().await;
                let header_json = match serde_json::to_vec(&env.header) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!("serialize header: {e}");
                        continue;
                    }
                };
                if let Err(e) = write_frame_async(&mut *w, &header_json).await {
                    tracing::error!("write header frame: {e}");
                    break;
                }
                let mut pcm_bytes = Vec::with_capacity(env.pcm.len() * 4);
                for s in env.pcm {
                    pcm_bytes.extend_from_slice(&s.to_le_bytes());
                }
                if let Err(e) = write_frame_async(&mut *w, &pcm_bytes).await {
                    tracing::error!("write pcm frame: {e}");
                    break;
                }
            }
        });

        // 3. Spawn reader task: drains stdout, dispatches to pending map.
        let pending: Arc<Mutex<HashMap<String, ResponseTx>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_for_reader = pending.clone();
        tokio::spawn(async move {
            let mut stdout = stdout;
            loop {
                let frame = match read_frame_async(&mut stdout).await {
                    Ok(Some(f)) => f,
                    Ok(None) => {
                        tracing::info!("stt_worker stdout EOF");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("read response frame: {e}");
                        break;
                    }
                };
                let resp: ResponseMessage = match serde_json::from_slice(&frame) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("malformed response JSON: {e}");
                        continue;
                    }
                };
                let id = resp.request_id.clone();
                let tx = pending_for_reader.lock().remove(&id);
                if let Some(tx) = tx {
                    let outcome = match TranscriptionResult::from_response(resp) {
                        Ok(r) => Ok(r),
                        Err(e) => Err(SttClientError::Worker(e)),
                    };
                    let _ = tx.send(outcome);
                } else {
                    tracing::warn!("response for unknown request_id {id}");
                }
            }
            // Worker died — fail every pending request.
            let mut map = pending_for_reader.lock();
            for (_, tx) in map.drain() {
                let _ = tx.send(Err(SttClientError::Closed));
            }
        });

        Ok((
            SttClient {
                inner: Arc::new(ClientInner { stdin_tx, pending }),
            },
            ready,
        ))
    }

    /// Submit one utterance for transcription. The future resolves when the
    /// worker returns a response for this `request_id`, or fails if the worker
    /// dies in the meantime.
    pub async fn transcribe(
        &self,
        request_id: String,
        samples: Vec<f32>,
        language: &str,
        initial_prompt: &str,
    ) -> Result<TranscriptionResult, SttClientError> {
        let header = RequestHeader {
            request_id: request_id.clone(),
            sample_rate: 16_000,
            language: language.to_string(),
            initial_prompt: initial_prompt.to_string(),
            n_samples: samples.len() as u32,
        };
        let (resp_tx, resp_rx) = oneshot::channel();
        self.inner.pending.lock().insert(request_id, resp_tx);
        self.inner
            .stdin_tx
            .send(RequestEnvelope { header, pcm: samples })
            .await
            .map_err(|_| SttClientError::Closed)?;
        resp_rx.await.map_err(|_| SttClientError::Closed)?
    }
}
