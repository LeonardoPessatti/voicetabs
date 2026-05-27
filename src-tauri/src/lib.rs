pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod hallucination;
pub mod hotkey;
pub mod logging;
pub mod paths;
pub mod routing;
pub mod stt;
pub mod tray;
pub mod utterance;
pub mod vad;
pub mod vocab;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::capture::{CaptureController, CaptureStatus, UtteranceFinalized};
use crate::db::{segments as segments_repo, Db};
use crate::hallucination::{evaluate, Decision, Input as HInput, Thresholds};
use crate::stt::active::{build_backend, ActiveBackend, BackendKind};
use crate::stt::local::LocalSttBackend;
use crate::stt::status::SttStatus;
use crate::stt::{SttStatusHandle, SttSupervisor, SupervisorConfig};
use crate::vocab::build_initial_prompt;

pub fn run() {
    let _guard = match paths::log_dir().and_then(logging::init) {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("logging init failed: {e}");
            None
        }
    };

    let db_path = paths::app_data_dir()
        .expect("app data dir")
        .join("voicetabs.db");
    let db = db::open(&db_path).expect("open db");

    let audio_dir = paths::audio_dir().expect("audio dir");
    let active_tab = routing::ActiveTab::new();

    // Capture mode is loaded from settings (persisted across runs). The
    // handle is `Send + Sync + Clone`: held by the Tauri state, the
    // controller worker, and the `capture_set_mode` command.
    let mode_handle = capture::CaptureModeHandle::new(
        load_capture_mode(&db).unwrap_or(capture::CaptureMode::AlwaysOn),
    );

    // Initial backend kind comes from the persisted `stt_backend` setting.
    // Missing/garbage values fall back to Local. The supervisor's identifier
    // for Local stays "cpu" for telemetry symmetry with earlier phases.
    let initial_kind: BackendKind = db::settings::get::<String>(&db, "stt_backend")
        .ok()
        .flatten()
        .map(|s| BackendKind::from_setting(&s))
        .unwrap_or(BackendKind::Local);
    let backend = match initial_kind {
        BackendKind::Local => "cpu".to_string(),
        BackendKind::Openai => "openai".to_string(),
    };

    let stt_status = SttStatusHandle::new(SttStatus::Loading {
        backend: backend.clone(),
    });
    let stt_status_for_state = stt_status.clone();

    // Clone everything we need to move into the `setup` closure so the
    // original handles remain available for the state slots above.
    let db_for_setup = db.clone();
    let active_tab_for_setup = active_tab.clone();
    let mode_handle_for_setup = mode_handle.clone();
    let audio_dir_for_setup = audio_dir.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(db.clone())
        .manage(active_tab.clone())
        .manage(stt_status_for_state)
        .manage(mode_handle.clone())
        .invoke_handler(tauri::generate_handler![
            commands::tabs::tabs_list,
            commands::tabs::tabs_create,
            commands::tabs::tabs_rename,
            commands::tabs::tabs_delete,
            commands::tabs::tabs_reorder,
            commands::tabs::tabs_set_active,
            commands::segments::segments_list_for_tab,
            commands::segments::segments_update,
            commands::segments::segments_delete,
            commands::segments::segments_retranscribe,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::capture::capture_start,
            commands::capture::capture_stop,
            commands::capture::capture_status,
            commands::capture::capture_get_mode,
            commands::capture::capture_set_mode,
            commands::hotkey::hotkey_get_binding,
            commands::hotkey::hotkey_set_binding,
            commands::hotkey::hotkey_clear_binding,
            commands::hotkey::hotkey_capture_next,
            commands::stt::stt_status,
            commands::backend::backend_get,
            commands::backend::backend_set,
            commands::backend::openai_key_set,
            commands::backend::openai_key_clear,
            commands::backend::openai_key_status,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // Mount the hotkey manager and restore the persisted binding (if
            // any). Failures registering the binding are logged but
            // non-fatal — the user can re-bind from the UI.
            let hotkey_mgr = hotkey::HotkeyManager::new(app_handle.clone())
                .expect("init hotkey manager");
            if let Ok(Some(json)) =
                db::settings::get::<serde_json::Value>(&db_for_setup, "hotkey_binding")
            {
                if let Ok(b) = serde_json::from_value::<hotkey::Binding>(json) {
                    if let Err(e) = hotkey_mgr.set_binding(b) {
                        tracing::warn!("failed to register persisted hotkey binding: {e}");
                    }
                }
            }
            let hotkey_rx = hotkey_mgr.subscribe();
            app.manage(hotkey_mgr);

            // Now spawn the capture controller with the real mode handle and
            // hotkey receiver.
            let capture = capture::CaptureController::spawn(
                audio_dir_for_setup.clone(),
                db_for_setup.clone(),
                active_tab_for_setup.clone(),
                mode_handle_for_setup.clone(),
                hotkey_rx,
            );
            let utt_rx = capture.utterance_receiver();
            app.manage(capture);

            // Build the system tray (icon, menu, click handlers). The
            // `should_exit` flag is set by the Quit menu item and checked by
            // the window close handler installed below: when false, close
            // hides to tray; when true, close proceeds and the app exits.
            let tray = tray::TrayHandle::build(&app_handle).expect("build tray icon");
            let should_exit = tray.should_exit();
            app.manage(tray.clone());

            // Intercept close events on the main window: hide to tray instead
            // of quitting, unless the Quit menu item set `should_exit`.
            if let Some(window) = app_handle.get_webview_window("main") {
                let should_exit_for_window = should_exit.clone();
                let app_handle_for_window = app_handle.clone();
                window.on_window_event(move |evt| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = evt {
                        if !should_exit_for_window.load(std::sync::atomic::Ordering::SeqCst) {
                            api.prevent_close();
                            if let Some(w) = app_handle_for_window.get_webview_window("main") {
                                let _ = w.hide();
                            }
                        }
                    }
                });
            }

            // Reflect capture state on the tray icon. The capture controller
            // does not publish a stream of status changes today; we poll
            // cheaply once a second. Phase 6 may replace this with an event.
            {
                let tray_for_poll = tray.clone();
                let app_for_poll = app_handle.clone();
                std::thread::Builder::new()
                    .name("voicetabs-tray-poll".into())
                    .spawn(move || loop {
                        let capturing = app_for_poll
                            .try_state::<CaptureController>()
                            .map(|c| !matches!(c.status(), CaptureStatus::Idle))
                            .unwrap_or(false);
                        let _ = tray_for_poll.set_capturing(capturing);
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                    })
                    .expect("spawn tray poll thread");
            }

            let model_path = resolve_model_path(&app_handle).expect("resolve model path");
            let worker_binary =
                resolve_worker_binary(&app_handle).expect("resolve worker binary");

            let model_name = model_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("(unknown)");
            let model_size_mb = std::fs::metadata(&model_path)
                .map(|m| m.len() as f64 / (1024.0 * 1024.0))
                .unwrap_or(0.0);
            tracing::warn!(
                "\n\
                ═══════════════════════════════════════════════════════════════\n\
                  STT BACKEND:   {}\n\
                  Worker binary: {}\n\
                  Model:         {} ({:.0} MB)\n\
                ═══════════════════════════════════════════════════════════════",
                match initial_kind {
                    BackendKind::Local => "Local (CPU)",
                    BackendKind::Openai => "OpenAI (cloud)",
                },
                worker_binary.display(),
                model_name,
                model_size_mb,
            );

            let cfg = SupervisorConfig {
                worker_binary,
                model_path,
                language: "pt".into(),
                backend: backend.clone(),
            };
            let supervisor = SttSupervisor::new(cfg, stt_status.clone());
            app.manage(supervisor.clone());

            // Wrap supervisor in LocalSttBackend (the SttBackend impl for
            // local subprocess), then seed ActiveBackend with whatever
            // initial_kind asked for. If the OpenAI key isn't configured
            // (or selection fails for any reason) we fall back to local
            // and log a warning — the user can swap from the UI later.
            let local_backend: Arc<LocalSttBackend> =
                Arc::new(LocalSttBackend::new(supervisor.clone()));
            let initial_arc: Arc<dyn crate::stt::backend::SttBackend> =
                match build_backend(initial_kind, local_backend.clone()) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(
                            "initial backend build failed ({e}); falling back to local"
                        );
                        local_backend.clone()
                    }
                };
            let active = ActiveBackend::new(initial_arc);
            app.manage(active.clone());
            app.manage(local_backend.clone());

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            // Leak the runtime to keep it alive for the app's lifetime. The
            // alternative — `manage(Arc<Runtime>)` — works too but adds an
            // extra `manage()` slot. A leaked runtime is fine for a desktop
            // app: it lives until the process exits.
            let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(rt));

            // Boot the supervisor.
            let sup_for_boot = supervisor.clone();
            rt.spawn(async move {
                if let Err(e) = sup_for_boot.boot().await {
                    tracing::error!("stt supervisor boot failed: {e}");
                }
            });

            // Drainer: consume utterances, run STT + RMS + hallucination
            // filter, insert kept segments into the DB, and emit
            // `segment-created` to the frontend. Routes transcribe calls
            // through `ActiveBackend` so a backend swap from the UI takes
            // effect on the next utterance.
            let sup_for_drainer = supervisor.clone();
            let active_for_drainer = active.clone();
            let app_for_drainer = app_handle.clone();
            let db_for_drainer = db.clone();
            rt.spawn(async move {
                drain_utterances(
                    utt_rx,
                    sup_for_drainer,
                    active_for_drainer,
                    app_for_drainer,
                    db_for_drainer,
                )
                .await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn load_capture_mode(db: &Db) -> Option<capture::CaptureMode> {
    db::settings::get::<String>(db, "capture_mode")
        .ok()
        .flatten()
        .and_then(|s| capture::CaptureMode::parse(&s))
}

fn resolve_model_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    // We bundle the `small` quantized model (~330 MB) for usable CPU
    // transcription. Cloud transcription via the OpenAI backend (Phase 7)
    // bypasses this entirely and uses `gpt-4o-mini-transcribe`.
    let p = app
        .path()
        .resolve(
            "resources/ggml-small-q5_1.bin",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| anyhow::anyhow!("resolve model path: {e}"))?;
    if !p.exists() {
        return Err(anyhow::anyhow!(
            "model file not found at {}; download from \
             https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
            p.display()
        ));
    }
    Ok(p)
}

fn resolve_worker_binary(_app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    // In dev mode, the worker lives next to voicetabs.exe under target/.
    // In a bundled installer it lives in the resource dir as an externalBin.
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        // Bundled: alongside the main exe.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                v.push(dir.join("stt_worker_cpu.exe"));
            }
        }
        // Dev: cargo workspace target.
        v.push(PathBuf::from("../target/debug/stt_worker_cpu.exe"));
        v.push(PathBuf::from("../target/release/stt_worker_cpu.exe"));
        v.push(PathBuf::from("target/debug/stt_worker_cpu.exe"));
        v.push(PathBuf::from("target/release/stt_worker_cpu.exe"));
        v
    };
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(anyhow::anyhow!(
        "could not locate stt_worker_cpu.exe; checked: {:?}",
        candidates
    ))
}

async fn drain_utterances(
    utt_rx: crossbeam_channel::Receiver<UtteranceFinalized>,
    sup: SttSupervisor,
    active: ActiveBackend,
    app: tauri::AppHandle,
    db: Db,
) {
    tracing::info!("drain_utterances: started");
    // crossbeam_channel is sync; we move blocking recv onto a dedicated thread
    // and forward into an async channel so the drainer can `await` without
    // blocking the runtime.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<UtteranceFinalized>();
    std::thread::Builder::new()
        .name("voicetabs-utt-bridge".into())
        .spawn(move || {
            tracing::info!("utt-bridge thread: started");
            while let Ok(u) = utt_rx.recv() {
                tracing::info!(
                    "utt-bridge: forwarding utterance audio={:?} tab={}",
                    u.audio_path,
                    u.start_tab_id,
                );
                if tx.send(u).is_err() {
                    tracing::warn!("utt-bridge: async receiver dropped, exiting");
                    break;
                }
            }
            tracing::warn!("utt-bridge: crossbeam channel closed, exiting");
        })
        .expect("spawn utt bridge");

    while let Some(u) = rx.recv().await {
        tracing::info!(
            "drain_utterances: received utterance audio={:?} tab={}, spawning pipeline",
            u.audio_path,
            u.start_tab_id,
        );
        let sup = sup.clone();
        let active = active.clone();
        let app = app.clone();
        let db = db.clone();
        // Spawn per-utterance so a slow transcription doesn't block the queue.
        tokio::spawn(async move {
            run_segment_pipeline(u, sup, active, app, db).await;
        });
    }
    tracing::warn!("drain_utterances: async receiver closed, exiting");
}

/// The Phase-4 finalize → STT → RMS → filter → insert → emit pipeline. Lives
/// in `lib.rs` rather than `controller.rs` because `SttBackend::transcribe`
/// is async and the controller worker is a sync `std::thread`. Routes
/// through `ActiveBackend` so backend swaps from the UI take effect on the
/// next utterance.
async fn run_segment_pipeline(
    u: UtteranceFinalized,
    sup: SttSupervisor,
    active: ActiveBackend,
    app: tauri::AppHandle,
    db: Db,
) {
    use crate::stt::status::{SttStatus, SttStatusHandle};
    let status_handle: Option<SttStatusHandle> = app
        .try_state::<SttStatusHandle>()
        .map(|s| s.inner().clone());
    let UtteranceFinalized {
        audio_path,
        samples,
        start_tab_id,
        vocab_snapshot,
        language,
        started_at_ms,
        ended_at_ms,
    } = u;

    // 1. RMS on the same samples we wrote to disk. Cheap; runs before STT so
    //    we have it whether STT succeeds or not.
    let rms_dbfs = crate::audio::rms_dbfs(&samples);
    tracing::info!(
        "segment_pipeline: tab={} audio={:?} samples={} rms_dbfs={:.1}",
        start_tab_id,
        audio_path,
        samples.len(),
        rms_dbfs,
    );

    // 2. Build initial_prompt and call STT through the active backend.
    let initial_prompt = build_initial_prompt(&vocab_snapshot, &language);
    let request_id = uuid::Uuid::new_v4().to_string();
    // Snapshot the active backend BEFORE awaiting transcribe so a
    // mid-call swap doesn't kill this one.
    let backend = active.snapshot().await;
    tracing::info!(
        "segment_pipeline: calling {}.transcribe request_id={} initial_prompt={:?}",
        backend.backend_id(),
        request_id,
        initial_prompt,
    );
    let req = crate::stt::backend::TranscribeRequest {
        request_id: request_id.clone(),
        samples,
        sample_rate: 16_000,
        language: language.clone(),
        initial_prompt: initial_prompt.clone(),
        audio_path: Some(audio_path.clone()),
        started_at_ms,
    };
    let result = match backend.transcribe(req).await {
        Ok(r) => {
            tracing::info!(
                "segment_pipeline: transcribe OK request_id={} text={:?} no_speech_prob={:.3} avg_logprob={:.3} duration_ms={}",
                request_id,
                r.text,
                r.no_speech_prob,
                r.avg_logprob,
                r.duration_ms,
            );
            // Restore Ready in case a previous failure left status=Error.
            if let Some(sh) = &status_handle {
                if matches!(sh.get(), SttStatus::Error { .. }) {
                    sh.set(SttStatus::Ready {
                        backend: backend.backend_id().into(),
                        model_id: backend.model_id().into(),
                    });
                }
            }
            r
        }
        Err(e) => {
            tracing::warn!(
                "stt transcribe failed for {}: {e}; keeping WAV at {:?}",
                request_id,
                audio_path,
            );
            // Surface to the UI so the status dot flips red instead of the
            // failure being log-only. The next successful utterance will
            // overwrite this back to Ready.
            if let Some(sh) = &status_handle {
                sh.set(SttStatus::Error { message: e.to_string() });
            }
            return;
        }
    };

    // 3. Hallucination filter.
    let decision = evaluate(
        &HInput {
            text: &result.text,
            avg_logprob: result.avg_logprob,
            no_speech_prob: result.no_speech_prob,
            rms_dbfs,
        },
        &Thresholds::default(),
    );
    let kept_text = match decision {
        Decision::Keep => result.text,
        Decision::Drop(reason) => {
            tracing::info!(
                "hallucination filter dropped utterance at {:?}: {:?} \
                 (text={:?}, no_speech={}, logprob={}, rms_db={})",
                audio_path,
                reason,
                result.text,
                result.no_speech_prob,
                result.avg_logprob,
                rms_dbfs,
            );
            return;
        }
    };

    // 4. Persist. The `audio_path` column is the file name only (relative to
    //    %APPDATA%\voicetabs\audio\), per spec §6.1.
    let file_name = match audio_path.file_name().and_then(|s| s.to_str()) {
        Some(n) => n.to_string(),
        None => {
            tracing::error!("could not extract file name from {:?}", audio_path);
            return;
        }
    };
    let vocab_snapshot_json =
        serde_json::to_string(&vocab_snapshot).unwrap_or_else(|_| "[]".into());
    let duration_ms = ended_at_ms.saturating_sub(started_at_ms) as i64;

    // For OpenAI we trust the backend's static model_id (`gpt-4o-mini-transcribe`).
    // For Local the supervisor's TranscriptionResult does not carry model_id
    // (the worker reports it once at handshake via ReadyMessage), so we
    // read it from the SttStatusHandle which the supervisor publishes into
    // on every (re)boot. If the worker isn't Ready (shouldn't happen after
    // a successful transcribe, but be defensive), fall back to a literal.
    let model_id = if backend.backend_id() == "openai" {
        backend.model_id().to_string()
    } else {
        match sup.status_handle().get() {
            SttStatus::Ready { model_id, .. } => model_id,
            _ => "unknown".into(),
        }
    };

    let new = segments_repo::NewSegment {
        tab_id: start_tab_id,
        text: kept_text,
        audio_path: file_name,
        started_at: started_at_ms as i64,
        ended_at: ended_at_ms as i64,
        duration_ms,
        vocab_snapshot: vocab_snapshot_json,
        avg_logprob: result.avg_logprob as f64,
        no_speech_prob: result.no_speech_prob as f64,
        model_id,
    };
    let inserted = match segments_repo::insert(&db, &new) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                "segments_repo::insert failed: {e}; orphan WAV at {:?}",
                audio_path,
            );
            return;
        }
    };

    // 5. Notify the frontend. The Phase-4 `segmentsStore` listens.
    if let Err(e) = app.emit("segment-created", &inserted) {
        tracing::warn!("failed to emit segment-created: {e}");
    }
}
