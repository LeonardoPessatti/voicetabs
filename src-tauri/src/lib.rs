pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;
pub mod stt;
pub mod utterance;
pub mod vad;

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

    tauri::Builder::default()
        .manage(db)
        .manage(capture)
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
        ])
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
