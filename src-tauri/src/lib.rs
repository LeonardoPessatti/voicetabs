pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod hallucination;
pub mod logging;
pub mod paths;
pub mod routing;
pub mod stt;
pub mod utterance;
pub mod vad;
pub mod vocab;

use std::path::PathBuf;

use tauri::{Emitter, Manager};

use crate::capture::UtteranceFinalized;
use crate::db::{segments as segments_repo, Db};
use crate::hallucination::{evaluate, Decision, Input as HInput, Thresholds};
use crate::stt::gpu::Backend;
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
    let capture = capture::CaptureController::spawn(audio_dir, db.clone(), active_tab.clone());
    let utt_rx = capture.utterance_receiver();

    // GPU autodetect (sync; uses settings cache).
    let backend = stt::gpu::detect_or_load(&db);

    let stt_status = SttStatusHandle::new(SttStatus::Loading {
        backend: backend.as_str().to_string(),
    });
    let stt_status_for_state = stt_status.clone();

    tauri::Builder::default()
        .manage(db.clone())
        .manage(capture)
        .manage(active_tab.clone())
        .manage(stt_status_for_state)
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
            commands::stt::stt_status,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let model_path = resolve_model_path(&app_handle).expect("resolve model path");
            let worker_binary =
                resolve_worker_binary(&app_handle, backend).expect("resolve worker binary");
            let cfg = SupervisorConfig {
                worker_binary,
                model_path,
                language: "pt".into(),
                backend,
            };
            let supervisor = SttSupervisor::new(cfg, stt_status.clone());
            app.manage(supervisor.clone());

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
            // `segment-created` to the frontend.
            let sup_for_drainer = supervisor.clone();
            let app_for_drainer = app_handle.clone();
            let db_for_drainer = db.clone();
            rt.spawn(async move {
                drain_utterances(utt_rx, sup_for_drainer, app_for_drainer, db_for_drainer).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn resolve_model_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let p = app
        .path()
        .resolve(
            "resources/ggml-large-v3-turbo-q5_0.bin",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| anyhow::anyhow!("resolve model path: {e}"))?;
    if !p.exists() {
        return Err(anyhow::anyhow!(
            "model file not found at {}; did you run the Task 2 download?",
            p.display()
        ));
    }
    Ok(p)
}

fn resolve_worker_binary(_app: &tauri::AppHandle, backend: Backend) -> anyhow::Result<PathBuf> {
    let name = match backend {
        Backend::Cuda => "stt_worker_cuda",
        Backend::Cpu => "stt_worker_cpu",
    };
    // In dev mode, the worker lives next to voicetabs.exe under target/.
    // In a bundled installer it lives in the resource dir as an externalBin.
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        // Bundled: alongside the main exe.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                v.push(dir.join(format!("{name}.exe")));
            }
        }
        // Dev: cargo workspace target.
        v.push(PathBuf::from(format!("../target/debug/{name}.exe")));
        v.push(PathBuf::from(format!("../target/release/{name}.exe")));
        v.push(PathBuf::from(format!("target/debug/{name}.exe")));
        v.push(PathBuf::from(format!("target/release/{name}.exe")));
        v
    };
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(anyhow::anyhow!(
        "could not locate {name}.exe; checked: {:?}",
        candidates
    ))
}

async fn drain_utterances(
    utt_rx: crossbeam_channel::Receiver<UtteranceFinalized>,
    sup: SttSupervisor,
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
        let app = app.clone();
        let db = db.clone();
        // Spawn per-utterance so a slow transcription doesn't block the queue.
        tokio::spawn(async move {
            run_segment_pipeline(u, sup, app, db).await;
        });
    }
    tracing::warn!("drain_utterances: async receiver closed, exiting");
}

/// The Phase-4 finalize → STT → RMS → filter → insert → emit pipeline. Lives
/// in `lib.rs` rather than `controller.rs` because `SttSupervisor::transcribe`
/// is async and the controller worker is a sync `std::thread`.
async fn run_segment_pipeline(
    u: UtteranceFinalized,
    sup: SttSupervisor,
    app: tauri::AppHandle,
    db: Db,
) {
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

    // 2. Build initial_prompt and call STT.
    let initial_prompt = build_initial_prompt(&vocab_snapshot, &language);
    let request_id = uuid::Uuid::new_v4().to_string();
    tracing::info!(
        "segment_pipeline: calling supervisor.transcribe request_id={} initial_prompt={:?}",
        request_id,
        initial_prompt,
    );
    let result = match sup
        .transcribe(
            request_id.clone(),
            samples,
            &language,
            &initial_prompt,
            Some(audio_path.clone()),
            started_at_ms,
        )
        .await
    {
        Ok(r) => {
            tracing::info!(
                "segment_pipeline: transcribe OK request_id={} text={:?} no_speech_prob={:.3} avg_logprob={:.3} duration_ms={}",
                request_id,
                r.text,
                r.no_speech_prob,
                r.avg_logprob,
                r.duration_ms,
            );
            r
        }
        Err(e) => {
            tracing::warn!(
                "stt transcribe failed for {}: {e}; keeping WAV at {:?}",
                request_id,
                audio_path,
            );
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

    // The supervisor's TranscriptionResult does not carry model_id (the
    // worker reports it once at handshake via ReadyMessage). Pull it from
    // the SttStatusHandle, which the supervisor publishes into on every
    // (re)boot. If the worker isn't Ready right now (shouldn't happen after
    // a successful transcribe, but be defensive), fall back to a literal.
    let model_id = match sup.status_handle().get() {
        SttStatus::Ready { model_id, .. } => model_id,
        _ => "unknown".into(),
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
