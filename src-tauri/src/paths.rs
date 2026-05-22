use std::path::PathBuf;

use directories::BaseDirs;

/// Returns the per-user app data directory: `%APPDATA%\voicetabs\` on Windows.
///
/// We use `BaseDirs::data_dir()` (which is `%APPDATA%` on Windows) and append
/// our own `voicetabs` segment. We deliberately avoid `ProjectDirs` because it
/// forces a `\<org>\<app>\data` suffix that does not match the design layout.
pub fn app_data_dir() -> anyhow::Result<PathBuf> {
    let base = BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("could not resolve base directories"))?;
    let dir = base.data_dir().join("voicetabs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn log_dir() -> anyhow::Result<PathBuf> {
    Ok(app_data_dir()?.join("logs"))
}

pub fn audio_dir() -> anyhow::Result<PathBuf> {
    let dir = app_data_dir()?.join("audio");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
