use std::fs;

use tempfile::tempdir;

#[test]
fn init_creates_log_dir_and_emits_line() {
    let dir = tempdir().unwrap();
    let guard = voicetabs_lib::logging::init(dir.path().to_path_buf())
        .expect("init should succeed");
    tracing::info!("test line");
    drop(guard);

    let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert!(!entries.is_empty(), "log file should exist");
}
