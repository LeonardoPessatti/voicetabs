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

    tauri::Builder::default()
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    drop(_guard);
}
