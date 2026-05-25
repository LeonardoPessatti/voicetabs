use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::{select, unbounded, Receiver, Sender};
use parking_lot::Mutex;
use serde::Serialize;

use crate::audio::{spawn_input_stream, AudioConfig, InputStreamHandle};
use crate::capture::mode::{CaptureMode, CaptureModeHandle};
use crate::db::{settings as settings_repo, Db};
use crate::hotkey::HotkeyEvent;
use crate::routing::ActiveTab;
use crate::utterance::{builder::UtteranceConfig, FinalizedUtterance, UtteranceBuilder};
use crate::vad::{VadEvent, VadModel, VadStateMachine, CHUNK_SAMPLES};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptureStatus {
    Idle,
    Capturing { device_name: Option<String> },
    Error { message: String },
}

/// Emitted by the worker thread for every finalized utterance.
///
/// Carries the snapshots taken at the VAD rising edge (`start_tab_id`,
/// `vocab_snapshot`, `language`) alongside the WAV path + in-memory samples.
/// The downstream async drainer in `lib.rs` consumes these, runs STT + RMS
/// + the hallucination filter, then either inserts a segment row + emits
///   `segment-created` or drops with a logged reason. The WAV remains on disk
///   either way so dropped utterances are diagnosable.
#[derive(Debug, Clone)]
pub struct UtteranceFinalized {
    pub audio_path: PathBuf,
    pub samples: Vec<f32>,
    pub start_tab_id: i64,
    pub vocab_snapshot: Vec<String>,
    pub language: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

/// Snapshotted at each VAD rising edge. Pass-by-value (no `&self`) throughout
/// the rising-edge → finalize chain so nothing can silently re-read state
/// mid-utterance. This is the entire mechanism that protects A3.
#[derive(Debug, Clone)]
struct UtteranceMeta {
    start_tab_id: i64,
    vocab_terms: Vec<String>,
    language: String,
}

enum Cmd {
    Start,
    Stop,
}

/// Public capture controller, held inside Tauri's `manage()` slot.
/// `Send + Sync` — the cpal `Stream` itself lives only on the worker thread.
pub struct CaptureController {
    cmd_tx: Sender<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    utterance_rx: Receiver<UtteranceFinalized>,
}

impl CaptureController {
    /// Spawn the worker thread and return the controller handle. The worker
    /// runs for the lifetime of the process; we never join it.
    ///
    /// `db` is used to read the `vocab_terms` / `language` settings at each
    /// VAD rising edge (snapshot semantics). `active_tab` is read once at
    /// each rising edge as well; whatever the user does mid-utterance cannot
    /// change the routing.
    pub fn spawn(
        output_dir: PathBuf,
        db: Db,
        active_tab: ActiveTab,
        mode: CaptureModeHandle,
        hotkey_rx: Receiver<HotkeyEvent>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let (utt_tx, utt_rx) = unbounded::<UtteranceFinalized>();
        let status = Arc::new(Mutex::new(CaptureStatus::Idle));
        let status_for_worker = status.clone();
        std::thread::Builder::new()
            .name("voicetabs-capture".into())
            .spawn(move || {
                worker_loop(
                    cmd_rx,
                    status_for_worker,
                    output_dir,
                    db,
                    active_tab,
                    mode,
                    hotkey_rx,
                    utt_tx,
                )
            })
            .expect("spawn capture thread");
        Self {
            cmd_tx,
            status,
            utterance_rx: utt_rx,
        }
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

    /// Subscribe to utterance finalizations. The receiver is cheap to clone
    /// because crossbeam-channel receivers are MPMC.
    pub fn utterance_receiver(&self) -> Receiver<UtteranceFinalized> {
        self.utterance_rx.clone()
    }
}

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    cmd_rx: Receiver<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    output_dir: PathBuf,
    db: Db,
    active_tab: ActiveTab,
    mode: CaptureModeHandle,
    hotkey_rx: Receiver<HotkeyEvent>,
    utt_tx: Sender<UtteranceFinalized>,
) {
    let mut stream_handle: Option<InputStreamHandle> = None;
    let mut vad_model: Option<VadModel> = None;
    let mut vad_sm = VadStateMachine::new(32);
    let mut builder = UtteranceBuilder::new(UtteranceConfig::default(), output_dir);
    let mut accumulator: Vec<f32> = Vec::with_capacity(CHUNK_SAMPLES * 2);
    // The A3-defining state: this is `Some(meta)` for the lifetime of one
    // utterance. Set on rising edge; consumed on finalize.
    let mut current_meta: Option<UtteranceMeta> = None;

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
                            current_meta = None;
                            *status.lock() = CaptureStatus::Idle;
                        }
                        Err(_) => return,
                    }
                }
                recv(frames_rx) -> frame => {
                    let Ok(frame) = frame else { continue };
                    // Read the device's native config from the handle. The
                    // handle is borrowed for the lifetime of this block only;
                    // we read u32/u16 by value so no lasting borrow remains.
                    let (src_rate, src_channels) = match &stream_handle {
                        Some(h) => (h.device_sample_rate, h.device_channels),
                        None => continue, // stream stopped concurrently
                    };
                    // Downmix to mono and resample to 16 kHz before VAD.
                    let frame_16k = crate::audio::downmix_and_resample(
                        &frame,
                        src_rate,
                        src_channels,
                        16_000,
                    );
                    if frame_16k.is_empty() {
                        continue;
                    }
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
                    accumulator.extend_from_slice(&frame_16k);
                    process_chunks(
                        &mut accumulator,
                        model,
                        &mut vad_sm,
                        &mut builder,
                        &mut current_meta,
                        &db,
                        &active_tab,
                        &mode,
                        &utt_tx,
                    );
                }
                recv(hotkey_rx) -> evt => {
                    let Ok(evt) = evt else { continue };
                    if mode.get() != CaptureMode::Ptt {
                        // AlwaysOn: hotkey is informational only; drop.
                        continue;
                    }
                    let ts_ms = unix_now_ms();
                    match evt {
                        HotkeyEvent::Press => {
                            if current_meta.is_some() {
                                // already in an utterance; treat as auto-repeat
                                continue;
                            }
                            let Some(tab_id) = active_tab.snapshot() else {
                                tracing::warn!("PTT press with no active tab; ignoring");
                                continue;
                            };
                            let vocab_terms: Vec<String> =
                                settings_repo::get::<Vec<String>>(&db, "vocab_terms")
                                    .ok()
                                    .flatten()
                                    .unwrap_or_default();
                            let language: String = settings_repo::get::<String>(&db, "language")
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| "pt".into());
                            current_meta = Some(UtteranceMeta {
                                start_tab_id: tab_id,
                                vocab_terms,
                                language,
                            });
                            // Force the VAD state machine into Speaking so a
                            // VAD-driven FallingEdge in PTT mode is also
                            // suppressed (we drop it in process_chunks).
                            vad_sm.force_speaking();
                            // Synthesize a rising edge into the builder so its
                            // pre-roll bookkeeping runs.
                            let _ = builder.on_vad_event(VadEvent::RisingEdge { timestamp_ms: ts_ms });
                        }
                        HotkeyEvent::Release => {
                            let Some(meta) = current_meta.take() else {
                                // spurious release; nothing to finalize
                                continue;
                            };
                            if let Some(finalized) =
                                builder.on_vad_event(VadEvent::FallingEdge { timestamp_ms: ts_ms })
                            {
                                publish_utterance(&utt_tx, finalized, meta);
                            }
                            vad_sm.force_idle();
                        }
                    }
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
#[allow(clippy::too_many_arguments)]
fn process_chunks(
    accumulator: &mut Vec<f32>,
    model: &mut VadModel,
    sm: &mut VadStateMachine,
    builder: &mut UtteranceBuilder,
    current_meta: &mut Option<UtteranceMeta>,
    db: &Db,
    active_tab: &ActiveTab,
    mode: &CaptureModeHandle,
    utt_tx: &Sender<UtteranceFinalized>,
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
        // PTT mode: still observe VAD (so the model keeps running for any
        // future hallucination filter that wants prob) and still feed the
        // builder so the pre-roll buffer and max-duration cap apply, but do
        // NOT let VAD edges start or finalize an utterance — the hotkey is
        // the only authoritative boundary.
        if mode.get() == CaptureMode::Ptt {
            let _ = sm.observe(prob, ts_ms);
            let _ = builder.push_frame(&chunk);
            continue;
        }
        if let Some(event) = sm.observe(prob, ts_ms) {
            // *** A3-CRITICAL SECTION ***
            // On rising edge, snapshot the active tab id AND the vocab list +
            // language EXACTLY ONCE. From here until finalize, nothing reads
            // `active_tab` or `settings::vocab_terms` again. This is the
            // entire mechanism that protects A3.
            if matches!(event, VadEvent::RisingEdge { .. }) {
                let tab_id = active_tab.snapshot();
                let vocab_terms: Vec<String> =
                    settings_repo::get::<Vec<String>>(db, "vocab_terms")
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                // Settings.language is reserved for v1; hardcode "pt" for now.
                let language: String = settings_repo::get::<String>(db, "language")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "pt".into());
                if let Some(tab_id) = tab_id {
                    *current_meta = Some(UtteranceMeta {
                        start_tab_id: tab_id,
                        vocab_terms,
                        language,
                    });
                } else {
                    // No active tab. Don't even start an utterance — the
                    // builder would still write a WAV but we'd have nowhere
                    // to route the segment. Force the VAD back to idle and
                    // drop this chunk.
                    sm.force_idle();
                    tracing::warn!("rising edge with no active tab; dropping utterance");
                    continue;
                }
            }

            // Push the chunk BEFORE handling the event:
            //   - On RisingEdge: the chunk lands in the pre-roll first, then
            //     the event drains the pre-roll into the new current
            //     utterance, so the triggering chunk is included.
            //   - On FallingEdge: the chunk is appended to the current
            //     utterance first; the event finalizes including this chunk.
            let _ = builder.push_frame(&chunk);
            if let Some(finalized) = builder.on_vad_event(event) {
                if let Some(meta) = current_meta.take() {
                    publish_utterance(utt_tx, finalized, meta);
                }
            }
        } else if let Some(finalized) = builder.push_frame(&chunk) {
            // No edge event but the max-duration cap finalized a WAV. The
            // meta was captured at the original rising edge; consume it now.
            if let Some(meta) = current_meta.take() {
                publish_utterance(utt_tx, finalized, meta);
            }
            sm.force_idle();
        }
    }
}

fn publish_utterance(
    utt_tx: &Sender<UtteranceFinalized>,
    finalized: FinalizedUtterance,
    meta: UtteranceMeta,
) {
    tracing::info!(
        "wrote utterance WAV: {:?} (tab={}, {} vocab terms)",
        finalized.path,
        meta.start_tab_id,
        meta.vocab_terms.len(),
    );
    let _ = utt_tx.send(UtteranceFinalized {
        audio_path: finalized.path,
        samples: finalized.samples,
        start_tab_id: meta.start_tab_id,
        vocab_snapshot: meta.vocab_terms,
        language: meta.language,
        started_at_ms: finalized.started_at_ms,
        ended_at_ms: finalized.ended_at_ms,
    });
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
