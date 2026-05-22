use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::{select, unbounded, Receiver, Sender};
use parking_lot::Mutex;
use serde::Serialize;

use crate::audio::{spawn_input_stream, AudioConfig, InputStreamHandle};
use crate::utterance::{builder::UtteranceConfig, UtteranceBuilder};
use crate::vad::{VadModel, VadStateMachine, CHUNK_SAMPLES};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptureStatus {
    Idle,
    Capturing { device_name: Option<String> },
    Error { message: String },
}

enum Cmd {
    Start,
    Stop,
}

/// Public capture controller, held inside Tauri's `manage()` slot.
/// `Send + Sync` — the `Stream` itself lives only on the worker thread.
pub struct CaptureController {
    cmd_tx: Sender<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
}

impl CaptureController {
    /// Spawn the worker thread and return the controller handle. The worker
    /// runs for the lifetime of the process; we never join it.
    pub fn spawn(output_dir: PathBuf) -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let status = Arc::new(Mutex::new(CaptureStatus::Idle));
        let status_for_worker = status.clone();
        std::thread::Builder::new()
            .name("voicetabs-capture".into())
            .spawn(move || worker_loop(cmd_rx, status_for_worker, output_dir))
            .expect("spawn capture thread");
        Self { cmd_tx, status }
    }

    pub fn start(&self) {
        let _ = self.cmd_tx.send(Cmd::Start);
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(Cmd::Stop);
    }

    pub fn status(&self) -> CaptureStatus {
        self.status.lock().clone()
    }
}

fn worker_loop(
    cmd_rx: Receiver<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    output_dir: PathBuf,
) {
    let mut stream_handle: Option<InputStreamHandle> = None;
    let mut vad_model: Option<VadModel> = None;
    let mut vad_sm = VadStateMachine::new(32);
    let mut builder = UtteranceBuilder::new(UtteranceConfig::default(), output_dir);
    let mut accumulator: Vec<f32> = Vec::with_capacity(CHUNK_SAMPLES * 2);

    loop {
        // Clone the frames receiver out of the optional handle BEFORE the
        // select! so the borrow on `stream_handle` ends at this statement.
        // That lets the Stop arm reassign `stream_handle = None;` cleanly.
        let frames_rx = stream_handle.as_ref().map(|h| h.frames.clone());

        if let Some(frames_rx) = frames_rx {
            select! {
                recv(cmd_rx) -> cmd => {
                    match cmd {
                        Ok(Cmd::Start) => { /* already capturing */ }
                        Ok(Cmd::Stop) => {
                            stream_handle = None;
                            vad_sm.force_idle();
                            accumulator.clear();
                            *status.lock() = CaptureStatus::Idle;
                        }
                        Err(_) => return,
                    }
                }
                recv(frames_rx) -> frame => {
                    let Ok(frame) = frame else { continue };
                    // Lazy-load the VAD model the first time we need it.
                    if vad_model.is_none() {
                        match VadModel::new() {
                            Ok(m) => vad_model = Some(m),
                            Err(e) => {
                                tracing::error!("VAD model load failed: {e}");
                                *status.lock() = CaptureStatus::Error {
                                    message: format!("VAD load failed: {e}"),
                                };
                                stream_handle = None;
                                continue;
                            }
                        }
                    }
                    let model = vad_model.as_mut().expect("model loaded above");
                    accumulator.extend_from_slice(&frame);
                    process_chunks(&mut accumulator, model, &mut vad_sm, &mut builder);
                }
            }
        } else {
            // Idle: block on commands only.
            match cmd_rx.recv() {
                Ok(Cmd::Start) => match spawn_input_stream(AudioConfig::default(), 64) {
                    Ok(handle) => {
                        let device_name = crate::audio::default_input_name();
                        *status.lock() = CaptureStatus::Capturing { device_name };
                        stream_handle = Some(handle);
                    }
                    Err(e) => {
                        tracing::error!("capture start failed: {e}");
                        *status.lock() = CaptureStatus::Error {
                            message: e.to_string(),
                        };
                    }
                },
                Ok(Cmd::Stop) => { /* already stopped */ }
                Err(_) => return,
            }
        }
    }
}

/// Drain `accumulator` in 512-sample windows, running VAD + state machine +
/// utterance builder for each window. Leaves any remainder in place.
fn process_chunks(
    accumulator: &mut Vec<f32>,
    model: &mut VadModel,
    sm: &mut VadStateMachine,
    builder: &mut UtteranceBuilder,
) {
    while accumulator.len() >= CHUNK_SAMPLES {
        let chunk: Vec<f32> = accumulator.drain(..CHUNK_SAMPLES).collect();
        let prob = match model.predict(&chunk) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("VAD predict failed: {e}");
                continue;
            }
        };
        let ts_ms = unix_now_ms();
        if let Some(event) = sm.observe(prob, ts_ms) {
            // Push the chunk BEFORE handling the event:
            //   - On RisingEdge: the chunk lands in the pre-roll first, then
            //     the event drains the pre-roll into the new current
            //     utterance, so the triggering chunk is included.
            //   - On FallingEdge: the chunk is appended to the current
            //     utterance first; the event finalizes including this chunk.
            let _ = builder.push_frame(&chunk);
            if let Some(path) = builder.on_vad_event(event) {
                tracing::info!("wrote utterance WAV: {path:?}");
            }
        } else if let Some(path) = builder.push_frame(&chunk) {
            // No edge event but the max-duration cap finalized a WAV.
            tracing::info!("wrote (max-cap) utterance WAV: {path:?}");
            sm.force_idle();
        }
    }
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
