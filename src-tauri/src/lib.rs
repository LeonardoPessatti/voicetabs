pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod hallucination;
pub mod logging;
pub mod paths;
pub mod stt;
pub mod utterance;
pub mod vad;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::capture::UtteranceFinalized;
use crate::stt::gpu::Backend;
use crate::stt::status::SttStatus;
use crate::stt::{SttStatusHandle, SttSupervisor, SupervisorConfig};

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionEventPayload {
    pub request_id: String,
    pub text: String,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub duration_ms: u64,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub audio_path: String,
}

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
    let capture = capture::CaptureController::spawn(audio_dir);
    let utt_rx = capture.utterance_receiver();

    // GPU autodetect (sync; uses settings cache).
    let backend = stt::gpu::detect_or_load(&db);

    let stt_status = SttStatusHandle::new(SttStatus::Loading {
        backend: backend.as_str().to_string(),
    });
    let stt_status_for_state = stt_status.clone();

    tauri::Builder::default()
        .manage(db)
        .manage(capture)
        .manage(stt_status_for_state)
        .invoke_handler(tauri::generate_handler![
            commands::tabs::tabs_list,
            commands::tabs::tabs_create,
            commands::tabs::tabs_rename,
            commands::tabs::tabs_delete,
            commands::tabs::tabs_reorder,
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

            // Drainer: consume utterances, transcribe, emit Tauri events.
            let sup_for_drainer = supervisor.clone();
            let app_for_drainer = app_handle.clone();
            rt.spawn(async move {
                drain_utterances(utt_rx, sup_for_drainer, app_for_drainer).await;
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
) {
    // crossbeam_channel is sync; we move blocking recv onto a dedicated thread
    // and forward into an async channel.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<UtteranceFinalized>();
    std::thread::Builder::new()
        .name("voicetabs-utt-bridge".into())
        .spawn(move || {
            while let Ok(u) = utt_rx.recv() {
                if tx.send(u).is_err() {
                    break;
                }
            }
        })
        .expect("spawn utt bridge");

    while let Some(u) = rx.recv().await {
        let sup = sup.clone();
        let app = app.clone();
        // Spawn per-utterance so a slow transcription doesn't block the queue.
        tokio::spawn(async move {
            let samples = match read_wav_samples(&u.audio_path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("read wav {}: {e}", u.audio_path.display());
                    return;
                }
            };
            let request_id = uuid::Uuid::new_v4().to_string();
            match sup
                .transcribe(
                    request_id.clone(),
                    samples,
                    "pt",
                    "", // initial_prompt — populated from settings.vocab_terms in Phase 4
                    Some(u.audio_path.clone()),
                    u.started_at_ms,
                )
                .await
            {
                Ok(r) => {
                    tracing::info!(
                        "transcription request={} text={:?} ({}ms)",
                        r.request_id,
                        r.text,
                        r.duration_ms
                    );
                    let payload = TranscriptionEventPayload {
                        request_id: r.request_id,
                        text: r.text,
                        avg_logprob: r.avg_logprob,
                        no_speech_prob: r.no_speech_prob,
                        duration_ms: r.duration_ms,
                        started_at_ms: u.started_at_ms,
                        ended_at_ms: u.ended_at_ms,
                        audio_path: u.audio_path.to_string_lossy().to_string(),
                    };
                    if let Err(e) = app.emit("stt-transcription", &payload) {
                        tracing::warn!("emit stt-transcription failed: {e}");
                    }
                }
                Err(e) => {
                    tracing::error!("transcription failed for {}: {e}", request_id);
                }
            }
        });
    }
    let _ = Arc::<()>::new(()); // suppress unused import lint for Arc if needed
}

fn read_wav_samples(path: &std::path::Path) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 {
        return Err(anyhow::anyhow!(
            "unexpected wav spec: rate={} channels={}",
            spec.sample_rate,
            spec.channels
        ));
    }
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / 32_767.0))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(samples)
}
