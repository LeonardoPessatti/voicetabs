//! Integration test: spin up the stub worker, exercise the supervisor's happy
//! path + restart path.
//!
//! These tests intentionally do NOT exercise whisper-rs. The full WAV-to-text
//! round trip lives in `stt_worker/tests/round_trip.rs` (Task 15).
//!
//! The tests serialize via a process-global mutex because they mutate
//! `STUB_DIE_AFTER` / `STUB_DIE_MARKER` env vars, which would race if the two
//! tests ran in parallel inside the same test binary.

use std::path::{Path, PathBuf};

use tokio::sync::Mutex;

// Backend was an enum from the removed stt::gpu module; backend is now a String.
use voicetabs_lib::stt::status::SttStatus;
use voicetabs_lib::stt::{SttStatusHandle, SttSupervisor, SupervisorConfig};

// Async-aware mutex so the guard can be held across `.await` points without
// tripping clippy's `await_holding_lock`. These tests must serialize because
// they mutate `STUB_DIE_AFTER` / `STUB_DIE_MARKER` env vars, which would race
// if the two tests ran in parallel inside the same test binary.
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

fn stub_path() -> PathBuf {
    // The stub_worker binary is built alongside this test. Cargo places it in
    // the same directory as the test executable for the current profile.
    let mut p = std::env::current_exe().expect("current_exe");
    p.pop(); // drop the test binary name
    if p.ends_with("deps") {
        p.pop();
    }
    p.push(if cfg!(windows) { "stub_worker.exe" } else { "stub_worker" });
    assert!(p.exists(), "stub_worker not built; expected at {}", p.display());
    p
}

fn make_cfg() -> SupervisorConfig {
    SupervisorConfig {
        worker_binary: stub_path(),
        model_path: PathBuf::from("unused"), // stub ignores --model
        language: "pt".into(),
        backend: "cpu".into(),
    }
}

fn clear_stub_env() {
    std::env::remove_var("STUB_DIE_AFTER");
    std::env::remove_var("STUB_DIE_MARKER");
}

fn ensure_marker_absent(marker: &Path) {
    if marker.exists() {
        let _ = std::fs::remove_file(marker);
    }
}

#[tokio::test]
async fn happy_path_transcribes_once() {
    let _guard = ENV_LOCK.lock().await;
    clear_stub_env();

    let cfg = make_cfg();
    let status = SttStatusHandle::new(SttStatus::Loading { backend: "cpu".into() });
    let sup = SttSupervisor::new(cfg, status);
    sup.boot().await.expect("boot ok");

    let result = sup
        .transcribe("req-1".into(), vec![0.0_f32; 256], "pt", "", None, 0)
        .await
        .expect("transcribe ok");
    assert_eq!(result.request_id, "req-1");
    assert_eq!(result.text, "stub-ok");
}

#[tokio::test]
async fn worker_death_is_recovered_and_caller_succeeds_after_respawn() {
    let _guard = ENV_LOCK.lock().await;

    // The first stub child dies after one successful response and touches a
    // marker file. The respawned child sees the marker exists and stops
    // dying — that's how we test "supervisor recovers + caller can transcribe
    // again" without racing the test process to clear an env var.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let marker = tempdir.path().join("stub_died.flag");
    ensure_marker_absent(&marker);

    std::env::set_var("STUB_DIE_AFTER", "1");
    std::env::set_var("STUB_DIE_MARKER", &marker);

    let cfg = make_cfg();
    let status = SttStatusHandle::new(SttStatus::Loading { backend: "cpu".into() });
    let sup = SttSupervisor::new(cfg, status.clone());
    sup.boot().await.expect("boot ok");

    let _ = sup
        .transcribe("req-1".into(), vec![0.0_f32; 256], "pt", "", None, 0)
        .await
        .expect("first transcribe ok");

    // The watcher transitions status through Restarting -> Ready as it observes
    // the exit and respawns. Wait for that transition (budget: 5 s) so we know
    // the new child is up before we send req-2. Without this we'd see a stale
    // Ready that still refers to the dead first child.
    let mut saw_restarting = false;
    let mut respawned = false;
    for _ in 0..100 {
        match status.get() {
            SttStatus::Restarting { .. } => saw_restarting = true,
            SttStatus::Ready { .. } if saw_restarting => {
                respawned = true;
                break;
            }
            _ => {}
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        respawned,
        "supervisor did not respawn (saw_restarting={}): final status {:?}",
        saw_restarting,
        status.get()
    );
    assert!(marker.exists(), "expected first child to have touched the die marker");

    let r2 = sup
        .transcribe("req-2".into(), vec![0.0_f32; 256], "pt", "", None, 0)
        .await
        .expect("second transcribe ok after respawn");
    assert_eq!(r2.request_id, "req-2");
    assert_eq!(r2.text, "stub-ok");

    clear_stub_env();
}
