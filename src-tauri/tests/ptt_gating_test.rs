//! PTT path: a manually-driven hotkey channel produces an utterance with the
//! right tab id, even when VAD probabilities never cross the thresholds.

use std::time::Duration;

use voicetabs_lib::capture::{CaptureController, CaptureMode, CaptureModeHandle};
use voicetabs_lib::db::{self, tabs as tabs_repo};
use voicetabs_lib::hotkey::HotkeyEvent;
use voicetabs_lib::routing::ActiveTab;

#[test]
#[ignore = "requires audio device; run manually with --ignored"]
fn ptt_press_release_emits_utterance() {
    // This test is `#[ignore]` because the capture worker needs a real cpal
    // input device. The CI runner doesn't have one. Run locally with:
    //   cargo test --manifest-path src-tauri/Cargo.toml -- --ignored ptt_press_release_emits_utterance
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = db::open(&db_path).unwrap();
    let tab = tabs_repo::create(&db, "T1", 0).unwrap();
    let active = ActiveTab::new();
    active.set(tab.id);
    let mode = CaptureModeHandle::new(CaptureMode::Ptt);

    let (hk_tx, hk_rx) = crossbeam_channel::unbounded::<HotkeyEvent>();
    let ctrl = CaptureController::spawn(
        tmp.path().join("audio"),
        db.clone(),
        active.clone(),
        mode.clone(),
        hk_rx,
    );
    ctrl.start();
    std::thread::sleep(Duration::from_millis(300));

    hk_tx.send(HotkeyEvent::Press).unwrap();
    std::thread::sleep(Duration::from_millis(800)); // speak window
    hk_tx.send(HotkeyEvent::Release).unwrap();

    let utterance = ctrl
        .utterance_receiver()
        .recv_timeout(Duration::from_secs(2));
    assert!(
        utterance.is_ok(),
        "expected utterance within 2s of release, got {utterance:?}"
    );
    let u = utterance.unwrap();
    assert_eq!(u.start_tab_id, tab.id);
    assert!(!u.samples.is_empty(), "utterance should have buffered samples");
}
