pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;

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

    tauri::Builder::default()
        .manage(db)
        .invoke_handler(tauri::generate_handler![
            commands::tabs::tabs_list,
            commands::tabs::tabs_create,
            commands::tabs::tabs_rename,
            commands::tabs::tabs_delete,
            commands::tabs::tabs_reorder,
            commands::settings::settings_get,
            commands::settings::settings_set,
        ])
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    drop(_guard);
}
