//! Shared status published from the supervisor to the frontend via Tauri events
//! and to commands via a `manage()`-stored handle.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SttStatus {
    /// Worker process is spawning and the model is still loading.
    Loading { backend: String },
    /// Worker is ready; transcriptions can be issued.
    Ready {
        backend: String,
        model_id: String,
    },
    /// Worker died; supervisor is in the middle of respawning.
    Restarting { backend: String },
    /// A fatal error occurred (e.g. model file missing on disk).
    Error { message: String },
}

#[derive(Clone)]
pub struct SttStatusHandle {
    inner: Arc<Mutex<SttStatus>>,
}

impl SttStatusHandle {
    pub fn new(initial: SttStatus) -> Self {
        Self { inner: Arc::new(Mutex::new(initial)) }
    }

    pub fn get(&self) -> SttStatus {
        self.inner.lock().clone()
    }

    pub fn set(&self, s: SttStatus) {
        *self.inner.lock() = s;
    }
}
