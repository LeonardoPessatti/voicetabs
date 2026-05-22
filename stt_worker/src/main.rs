//! `stt_worker` entrypoint.
//!
//! Two binaries are produced from this same source file: `stt_worker_cpu.exe`
//! (default features) and `stt_worker_cuda.exe` (`--features cuda`). The only
//! observable difference is the `backend` field in the `ReadyMessage`.

use std::io::{stdin, stdout, BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use stt_worker::framing::{read_frame, write_frame};
use stt_worker::protocol::{ReadyMessage, RequestHeader, ResponseMessage};
use stt_worker::whisper::Engine;

#[cfg(feature = "cuda")]
const BACKEND_NAME: &str = "cuda";
#[cfg(not(feature = "cuda"))]
const BACKEND_NAME: &str = "cpu";

#[derive(Debug)]
struct Args {
    model: PathBuf,
    language: String,
    threads: i32,
}

fn parse_args() -> Result<Args, String> {
    let mut model: Option<PathBuf> = None;
    let mut language = "pt".to_string();
    let mut threads = num_cpus_default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--model" => {
                model = Some(PathBuf::from(it.next().ok_or("--model needs a value")?));
            }
            "--language" => {
                language = it.next().ok_or("--language needs a value")?;
            }
            "--threads" => {
                let v = it.next().ok_or("--threads needs a value")?;
                threads = v.parse::<i32>().map_err(|e| e.to_string())?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        model: model.ok_or("--model is required")?,
        language,
        threads,
    })
}

/// `num_cpus` would add a dependency; this approximates "logical cores - 1"
/// using `std::thread::available_parallelism`, clamped to [1, 16].
fn num_cpus_default() -> i32 {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    (logical - 1).clamp(1, 16)
}

fn init_tracing() {
    // Worker logs go to stderr so they don't corrupt the framed stdout.
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("VOICETABS_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true);
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .try_init();
}

fn run() -> Result<(), String> {
    init_tracing();
    let args = parse_args()?;
    tracing::info!(
        "loading model: {} (backend={}, default_language={}, threads={})",
        args.model.display(),
        BACKEND_NAME,
        args.language,
        args.threads
    );
    let mut engine = Engine::load(&args.model, args.threads).map_err(|e| e.to_string())?;
    tracing::info!("model loaded: {}", engine.model_id());

    let ready = ReadyMessage {
        ready: true,
        model_id: engine.model_id().to_string(),
        backend: BACKEND_NAME.to_string(),
    };
    let ready_json = serde_json::to_vec(&ready).map_err(|e| e.to_string())?;
    {
        let mut out = stdout().lock();
        write_frame(&mut out, &ready_json).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }

    let mut stdin_lock = BufReader::new(stdin().lock());

    loop {
        // 1. Header frame (JSON).
        let header_bytes = match read_frame(&mut stdin_lock) {
            Ok(Some(b)) => b,
            Ok(None) => {
                tracing::info!("stdin EOF — exiting cleanly");
                return Ok(());
            }
            Err(e) => return Err(format!("read header frame: {e}")),
        };
        let header: RequestHeader = match serde_json::from_slice(&header_bytes) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("malformed header JSON: {e}");
                continue;
            }
        };

        // 2. PCM frame.
        let pcm_bytes = match read_frame(&mut stdin_lock) {
            Ok(Some(b)) => b,
            Ok(None) => return Err("stdin EOF mid-request".into()),
            Err(e) => return Err(format!("read pcm frame: {e}")),
        };

        let expected_bytes = (header.n_samples as usize) * std::mem::size_of::<f32>();
        if pcm_bytes.len() != expected_bytes {
            let resp = ResponseMessage::err(
                header.request_id,
                format!(
                    "pcm size mismatch: header n_samples={} expects {} bytes, got {}",
                    header.n_samples,
                    expected_bytes,
                    pcm_bytes.len()
                ),
            );
            write_response(&resp)?;
            continue;
        }

        let samples: Vec<f32> = pcm_bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        // 3. Inference.
        let resp = match engine.transcribe(&samples, &header.language, &header.initial_prompt) {
            Ok(r) => ResponseMessage::ok(
                header.request_id,
                r.text,
                r.avg_logprob,
                r.no_speech_prob,
                r.duration_ms,
            ),
            Err(e) => ResponseMessage::err(header.request_id, e.to_string()),
        };

        write_response(&resp)?;
    }
}

fn write_response(resp: &ResponseMessage) -> Result<(), String> {
    let json = serde_json::to_vec(resp).map_err(|e| e.to_string())?;
    let mut out = stdout().lock();
    write_frame(&mut out, &json).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("fatal: {e}");
            eprintln!("stt_worker: fatal: {e}");
            ExitCode::from(1)
        }
    }
}
