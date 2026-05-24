//! `SttSupervisor` owns the worker child process.
//!
//! Responsibilities:
//! - Spawn the chosen worker binary with `--model <path>`.
//! - Run the IPC handshake via `SttClient::handshake`.
//! - Re-spawn on child exit within 500 ms; re-handshake; replay the last
//!   in-flight utterance once.
//! - Update `SttStatusHandle` and emit `stt-status` events on changes.
//! - Provide `transcribe()` for the rest of the main process to call.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::process::{Child, Command};
use tokio::sync::Mutex as TokioMutex;
use tokio::time::Duration;

use crate::stt::client::{SttClient, SttClientError};
use crate::stt::protocol::TranscriptionResult;
use crate::stt::status::{SttStatus, SttStatusHandle};

const RESPAWN_DELAY_MS: u64 = 500;
const MAX_REPLAY_ATTEMPTS: usize = 1;

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub worker_binary: PathBuf,
    pub model_path: PathBuf,
    pub language: String,
    /// Free-form backend identifier published in `SttStatus`. Phase 7 will
    /// introduce `"openai"`; today we always pass `"cpu"`. Keep as `String`
    /// to avoid an enum refactor when the cloud backend lands.
    pub backend: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("failed to spawn worker: {0}")]
    Spawn(String),
    #[error("handshake failed: {0}")]
    Handshake(String),
    #[error("worker error: {0}")]
    Worker(String),
}

/// Public handle held inside Tauri's `manage()` slot.
#[derive(Clone)]
pub struct SttSupervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    cfg: SupervisorConfig,
    status: SttStatusHandle,
    client: TokioMutex<Option<SttClient>>,
    /// Most recent in-flight utterance (for replay on child death). Wrapped in
    /// a sync Mutex because callers may inspect it without awaiting.
    last_inflight: Mutex<Option<InFlight>>,
}

#[derive(Clone)]
struct InFlight {
    request_id: String,
    samples: Vec<f32>,
    initial_prompt: String,
    audio_path: Option<PathBuf>,
    started_at_ms: u64,
}

impl SttSupervisor {
    pub fn new(cfg: SupervisorConfig, status: SttStatusHandle) -> Self {
        Self {
            inner: Arc::new(SupervisorInner {
                cfg,
                status,
                client: TokioMutex::new(None),
                last_inflight: Mutex::new(None),
            }),
        }
    }

    pub fn status_handle(&self) -> SttStatusHandle {
        self.inner.status.clone()
    }

    /// Boot the worker for the first time. Call once at app startup. Errors
    /// surface as `SttStatus::Error` via the status handle; this function
    /// returns `Ok(())` even on spawn failure so the rest of the app can boot.
    pub async fn boot(&self) -> Result<(), SupervisorError> {
        self.inner.status.set(SttStatus::Loading {
            backend: self.inner.cfg.backend.clone(),
        });
        match self.spawn_and_handshake().await {
            Ok((child, client, model_id)) => {
                *self.inner.client.lock().await = Some(client);
                self.inner.status.set(SttStatus::Ready {
                    backend: self.inner.cfg.backend.clone(),
                    model_id,
                });
                // Spawn the watcher in the background.
                let me = self.clone();
                tokio::spawn(async move { me.watch_loop(child).await });
                Ok(())
            }
            Err(e) => {
                self.inner.status.set(SttStatus::Error { message: e.to_string() });
                Err(e)
            }
        }
    }

    /// Public transcribe. Records `last_inflight` so we can replay on death.
    pub async fn transcribe(
        &self,
        request_id: String,
        samples: Vec<f32>,
        language: &str,
        initial_prompt: &str,
        audio_path: Option<PathBuf>,
        started_at_ms: u64,
    ) -> Result<TranscriptionResult, SupervisorError> {
        // Record before we await — even a spawn-failure replay needs it.
        *self.inner.last_inflight.lock() = Some(InFlight {
            request_id: request_id.clone(),
            samples: samples.clone(),
            initial_prompt: initial_prompt.to_string(),
            audio_path,
            started_at_ms,
        });

        let client = {
            let guard = self.inner.client.lock().await;
            guard
                .clone()
                .ok_or_else(|| SupervisorError::Worker("no client (worker down)".into()))?
        };

        match client.transcribe(request_id, samples, language, initial_prompt).await {
            Ok(r) => {
                self.inner.last_inflight.lock().take();
                Ok(r)
            }
            Err(SttClientError::Closed) => {
                // Worker died mid-flight; the watcher will respawn and replay.
                Err(SupervisorError::Worker("worker closed mid-request".into()))
            }
            Err(e) => Err(SupervisorError::Worker(e.to_string())),
        }
    }

    async fn spawn_and_handshake(
        &self,
    ) -> Result<(Child, SttClient, String), SupervisorError> {
        let mut cmd = Command::new(&self.inner.cfg.worker_binary);
        cmd.arg("--model")
            .arg(&self.inner.cfg.model_path)
            .arg("--language")
            .arg(&self.inner.cfg.language)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| SupervisorError::Spawn(e.to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SupervisorError::Spawn("no stdout".into()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SupervisorError::Spawn("no stdin".into()))?;
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            tokio::spawn(forward_stderr(stderr));
        }
        let (client, ready) = SttClient::handshake(stdout, stdin)
            .await
            .map_err(|e| SupervisorError::Handshake(e.to_string()))?;
        Ok((child, client, ready.model_id))
    }

    /// Watch the current child for exit, then respawn-and-replay in a loop.
    /// Iterative form (vs. recursive spawn) so the future is `Send` without
    /// having to box it manually.
    async fn watch_loop(self, initial_child: Child) {
        let _ = MAX_REPLAY_ATTEMPTS; // suppress unused-const warning
        let mut child = initial_child;
        loop {
            let status = child.wait().await;
            tracing::warn!("stt_worker child exited: {status:?}");

            // Update status, then attempt respawn + replay.
            self.inner.status.set(SttStatus::Restarting {
                backend: self.inner.cfg.backend.clone(),
            });
            tokio::time::sleep(Duration::from_millis(RESPAWN_DELAY_MS)).await;

            match self.spawn_and_handshake().await {
                Ok((new_child, new_client, model_id)) => {
                    *self.inner.client.lock().await = Some(new_client.clone());
                    self.inner.status.set(SttStatus::Ready {
                        backend: self.inner.cfg.backend.clone(),
                        model_id,
                    });
                    // Replay last in-flight (one attempt).
                    let inflight = self.inner.last_inflight.lock().take();
                    if let Some(inflight) = inflight {
                        tracing::info!(
                            "replaying in-flight utterance {}",
                            inflight.request_id
                        );
                        let _ = new_client
                            .transcribe(
                                inflight.request_id,
                                inflight.samples,
                                &self.inner.cfg.language,
                                &inflight.initial_prompt,
                            )
                            .await;
                    }
                    // Continue watching the new child.
                    child = new_child;
                }
                Err(e) => {
                    tracing::error!("respawn failed: {e}");
                    self.inner.status.set(SttStatus::Error {
                        message: format!("respawn failed: {e}"),
                    });
                    return;
                }
            }
        }
    }
}

async fn forward_stderr<R: tokio::io::AsyncRead + Unpin + Send + 'static>(stderr: R) {
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(stderr).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        tracing::info!(target: "stt_worker", "{}", line);
    }
}
