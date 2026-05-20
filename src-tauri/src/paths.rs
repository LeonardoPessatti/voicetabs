use std::path::PathBuf;

use directories::ProjectDirs;

/// Returns the per-user data directory: `%APPDATA%\voicetabs\` on Windows.
pub fn app_data_dir() -> anyhow::Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "voicetabs", "voicetabs")
        .ok_or_else(|| anyhow::anyhow!("could not resolve project dirs"))?;
    let dir = dirs.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn log_dir() -> anyhow::Result<PathBuf> {
    Ok(app_data_dir()?.join("logs"))
}
