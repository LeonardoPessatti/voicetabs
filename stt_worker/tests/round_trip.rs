//! Real subprocess integration test for `stt_worker_cpu.exe`.
//!
//! Spawns the actual worker binary, hands it a framed JSON header + f32 PCM
//! payload read from `src-tauri/tests/fixtures/speech_pt.wav`, and asserts that
//! a well-formed response frame comes back for the same `request_id`.
//!
//! The full round trip is tagged `#[ignore]` because it loads the ~574 MB
//! Whisper model (~5–10 s) and runs CPU inference on a 3 s clip (~1–5 s). Run
//! it explicitly with:
//!
//! ```text
//! cargo test -p stt_worker --test round_trip -- --ignored
//! ```
//!
//! The two non-ignored tests below run by default — they exercise the framing
//! helpers used by the round-trip test and confirm the fixture WAV is
//! well-formed, without paying the model-load cost.
//!
//! ## Fixture limitation (sine fallback)
//!
//! `src-tauri/tests/fixtures/speech_pt.wav` is currently a 3 s, 440 Hz sine
//! wave at 16 kHz mono 16-bit PCM — a deterministic stand-in that does not
//! depend on a TTS voice or a hand-recorded clip. Whisper will return an
//! empty / near-empty transcription with a high `no_speech_prob` for it, so
//! the ignored round-trip test only asserts on framing shape and the response
//! envelope, NOT on the transcribed text. The test prints whatever text comes
//! back so an engineer running `-- --ignored` can eyeball it.
//!
//! FIXME: replace `speech_pt.wav` with a real Brazilian Portuguese speech clip
//! (3–5 s, 16 kHz mono 16-bit PCM) when one becomes available. The
//! "transcribed text must not be empty" assertion can then be tightened.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` for this test = the `stt_worker/` crate dir. Its
    // parent is the workspace root.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn model_path() -> PathBuf {
    workspace_root().join("src-tauri/resources/ggml-large-v3-turbo-q5_0.bin")
}

fn fixture_wav() -> PathBuf {
    workspace_root().join("src-tauri/tests/fixtures/speech_pt.wav")
}

fn worker_bin() -> PathBuf {
    // Both the test binary and `stt_worker_cpu.exe` are built into
    // `target/<profile>/`. We resolve the profile from the current test
    // executable's location so this works for both `--release` and the
    // default debug profile.
    let mut p = std::env::current_exe().expect("current_exe");
    p.pop(); // drop test binary name
    if p.ends_with("deps") {
        p.pop();
    }
    p.push(if cfg!(windows) {
        "stt_worker_cpu.exe"
    } else {
        "stt_worker_cpu"
    });
    p
}

/// Read one length-prefixed frame from `r`. Returns `None` on clean EOF before
/// any bytes of the prefix have been read.
fn read_frame<R: Read>(r: &mut R) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    if r.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    if r.read_exact(&mut body).is_err() {
        return None;
    }
    Some(body)
}

/// Write one length-prefixed frame to `w`.
fn write_frame<W: Write>(w: &mut W, body: &[u8]) {
    let len = body.len() as u32;
    w.write_all(&len.to_le_bytes()).unwrap();
    w.write_all(body).unwrap();
    w.flush().unwrap();
}

/// Decode the fixture WAV into the f32 sample vector the worker expects.
fn read_wav(path: &std::path::Path) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).expect("open fixture wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "fixture must be 16 kHz");
    assert_eq!(spec.channels, 1, "fixture must be mono");
    assert_eq!(
        spec.bits_per_sample, 16,
        "fixture must be 16-bit PCM (got {} bits)",
        spec.bits_per_sample
    );
    reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32_767.0)
        .collect()
}

// ---------------------------------------------------------------------------
// Default (non-ignored) tests: cheap framing + fixture sanity checks.
// ---------------------------------------------------------------------------

/// Sanity-check that the committed fixture WAV is parseable and has the shape
/// the round-trip test expects. Cheap (no model load, no subprocess).
#[test]
fn fixture_wav_is_well_formed() {
    let path = fixture_wav();
    assert!(
        path.exists(),
        "fixture WAV missing at {} — generate via the PowerShell snippet in \
         docs/superpowers/plans/2026-05-22-voicetabs-phase-3-stt-subprocess.md Task 15 Step 1",
        path.display()
    );

    let samples = read_wav(&path);
    // 3 s @ 16 kHz mono = 48_000 samples (+/- a few from rounding).
    assert!(
        samples.len() >= 16_000 && samples.len() <= 16_000 * 6,
        "fixture should be 1–6 s of audio at 16 kHz; got {} samples (~{} s)",
        samples.len(),
        samples.len() as f32 / 16_000.0
    );
    // No NaN / inf in the decoded PCM.
    assert!(
        samples.iter().all(|s| s.is_finite()),
        "fixture WAV contains non-finite samples"
    );
}

/// Verify the local `read_frame` / `write_frame` helpers in this file behave
/// the way the round-trip test depends on (round-trip a JSON header and a
/// binary PCM payload through a `Cursor`).
#[test]
fn framing_helpers_round_trip_in_memory() {
    let header = br#"{"request_id":"req-1","sample_rate":16000,"language":"pt","initial_prompt":"","n_samples":4}"#;
    let pcm: Vec<u8> = [0.1_f32, 0.2, -0.3, 0.4]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();

    let mut buf = Vec::<u8>::new();
    write_frame(&mut buf, header);
    write_frame(&mut buf, &pcm);

    let mut cursor = std::io::Cursor::new(buf);
    let h = read_frame(&mut cursor).expect("header frame");
    let p = read_frame(&mut cursor).expect("pcm frame");
    assert_eq!(h, header);
    assert_eq!(p, pcm);
    // No third frame after EOF.
    assert!(read_frame(&mut cursor).is_none());
}

// ---------------------------------------------------------------------------
// Ignored: full subprocess round-trip (loads the real Whisper model).
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn round_trip_real_worker() {
    if !model_path().exists() {
        eprintln!(
            "skipping: model file not present at {}",
            model_path().display()
        );
        return;
    }
    if !fixture_wav().exists() {
        eprintln!(
            "skipping: fixture WAV not present at {}",
            fixture_wav().display()
        );
        return;
    }
    let bin = worker_bin();
    if !bin.exists() {
        panic!(
            "stt_worker_cpu.exe not built; run `cargo build -p stt_worker` first ({})",
            bin.display()
        );
    }

    let mut child = Command::new(&bin)
        .args([
            "--model",
            &model_path().to_string_lossy(),
            "--language",
            "pt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn worker");

    let stdin = child.stdin.take().expect("worker stdin");
    let stdout = child.stdout.take().expect("worker stdout");
    let stderr = child.stderr.take().expect("worker stderr");

    // Drain stderr in a thread so the worker never blocks on a full pipe.
    // Worker logs (model load, tracing output) flow through here.
    std::thread::spawn(move || {
        let r = BufReader::new(stderr);
        for line in r.lines().map_while(Result::ok) {
            eprintln!("[worker] {line}");
        }
    });

    let mut stdin = std::io::BufWriter::new(stdin);
    let mut stdout = std::io::BufReader::new(stdout);

    // 1. Read the ready frame the worker emits once the model is loaded.
    let ready_bytes = read_frame(&mut stdout).expect("ready frame");
    let ready: serde_json::Value =
        serde_json::from_slice(&ready_bytes).expect("ready JSON parse");
    assert_eq!(ready["ready"], serde_json::json!(true), "ready flag");
    assert!(
        ready["model_id"].is_string(),
        "model_id missing from ready frame: {ready}"
    );
    assert!(
        ready["backend"].is_string(),
        "backend missing from ready frame: {ready}"
    );

    // 2. Send the request: a JSON header frame followed by a raw f32 LE PCM
    //    frame. n_samples MUST match the byte count of the PCM frame divided
    //    by 4, or the worker returns an error response.
    let samples = read_wav(&fixture_wav());
    let header = format!(
        r#"{{"request_id":"req-1","sample_rate":16000,"language":"pt","initial_prompt":"","n_samples":{}}}"#,
        samples.len()
    );
    write_frame(&mut stdin, header.as_bytes());

    let mut pcm = Vec::with_capacity(samples.len() * 4);
    for s in &samples {
        pcm.extend_from_slice(&s.to_le_bytes());
    }
    write_frame(&mut stdin, &pcm);

    // Close stdin so the worker sees EOF after handling the request and exits
    // cleanly via its `Ok(None)` branch.
    drop(stdin);

    // 3. Read the response frame.
    let resp_bytes = read_frame(&mut stdout).expect("response frame");
    let resp: serde_json::Value =
        serde_json::from_slice(&resp_bytes).expect("response JSON parse");
    assert_eq!(resp["request_id"], serde_json::json!("req-1"));

    // 4. The fixture is a sine wave, so Whisper is allowed to return either
    //    an empty text (most likely) or some hallucinated content. What we do
    //    NOT tolerate is a structurally broken response (no text AND no
    //    error) or a hard error field surfaced by the worker.
    if let Some(err) = resp["error"].as_str() {
        panic!("worker returned error: {err}");
    }
    let text = resp["text"]
        .as_str()
        .unwrap_or_else(|| panic!("response had neither text nor error: {resp}"));
    eprintln!(
        "transcribed text: {text:?} (no_speech_prob={}, avg_logprob={}, duration_ms={})",
        resp["no_speech_prob"], resp["avg_logprob"], resp["duration_ms"]
    );
    if text.trim().is_empty() {
        eprintln!(
            "note: empty text is expected for the sine-wave fixture; replace \
             speech_pt.wav with a real speech clip to tighten this assertion"
        );
    }
    // Confidence-shape sanity: no_speech_prob must be in [0, 1].
    if let Some(p) = resp["no_speech_prob"].as_f64() {
        assert!(
            (0.0..=1.0).contains(&p),
            "no_speech_prob out of range: {p}"
        );
    }

    // 5. Worker should exit cleanly once stdin closes.
    let status = child.wait().expect("wait worker");
    assert!(status.success(), "worker exited with {status:?}");
}
