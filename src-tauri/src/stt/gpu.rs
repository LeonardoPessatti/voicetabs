//! GPU autodetection. Layered probe with caching in the `settings` table.
//!
//! Order:
//! 1. `WHISPER_CUDA` env var override (`"0"` = CPU, else CUDA).
//! 2. Cached `stt_backend` setting from `settings`.
//! 3. `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`.
//! 4. `wmic path win32_VideoController get name /format:list`.
//! 5. CPU.

use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::db::{settings as settings_repo, Db};

/// Minimum VRAM to treat an NVIDIA GPU as a CUDA candidate. Below this we'd
/// fall back to CPU rather than risk OOM with the large-v3-turbo model.
const MIN_VRAM_MIB: u32 = 4096;

const CACHE_KEY: &str = "stt_backend";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Cuda,
    Cpu,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Cuda => "cuda",
            Backend::Cpu => "cpu",
        }
    }
}

/// Public entry point. Reads the cache, otherwise probes and writes the cache.
pub fn detect_or_load(db: &Db) -> Backend {
    if let Some(b) = env_override() {
        tracing::info!("STT backend override from WHISPER_CUDA env: {}", b.as_str());
        return b;
    }
    match settings_repo::get::<Backend>(db, CACHE_KEY) {
        Ok(Some(b)) => {
            tracing::info!("STT backend from cache: {}", b.as_str());
            return b;
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("settings.get(stt_backend) failed: {e}"),
    }
    let chosen = probe();
    tracing::info!("STT backend probed: {}", chosen.as_str());
    if let Err(e) = settings_repo::set(db, CACHE_KEY, &chosen) {
        tracing::warn!("failed to cache stt_backend: {e}");
    }
    chosen
}

fn env_override() -> Option<Backend> {
    match std::env::var("WHISPER_CUDA").ok().as_deref() {
        None => None,
        Some("0") | Some("false") | Some("FALSE") | Some("False") => Some(Backend::Cpu),
        Some(_) => Some(Backend::Cuda),
    }
}

fn probe() -> Backend {
    if probe_nvidia_smi() {
        return Backend::Cuda;
    }
    if probe_wmic_nvidia() {
        return Backend::Cuda;
    }
    Backend::Cpu
}

fn probe_nvidia_smi() -> bool {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        // Lines look like "NVIDIA GeForce GTX 1060, 6144"
        let mut parts = line.splitn(2, ',');
        let _name = parts.next().unwrap_or("").trim();
        let mib_str = parts.next().unwrap_or("0").trim();
        if let Ok(mib) = mib_str.parse::<u32>() {
            if mib >= MIN_VRAM_MIB {
                return true;
            }
        }
    }
    false
}

fn probe_wmic_nvidia() -> bool {
    let out = Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "name", "/format:list"])
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    stdout.contains("nvidia")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial_schema.sql"))
            .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .unwrap();
        Db::from_connection(conn)
    }

    #[test]
    fn env_override_cpu() {
        std::env::set_var("WHISPER_CUDA", "0");
        assert_eq!(env_override(), Some(Backend::Cpu));
        std::env::remove_var("WHISPER_CUDA");
    }

    #[test]
    fn env_override_cuda_for_any_truthy_value() {
        std::env::set_var("WHISPER_CUDA", "1");
        assert_eq!(env_override(), Some(Backend::Cuda));
        std::env::set_var("WHISPER_CUDA", "yes");
        assert_eq!(env_override(), Some(Backend::Cuda));
        std::env::remove_var("WHISPER_CUDA");
    }

    #[test]
    fn cached_value_is_returned_without_reprobing() {
        let db = mem_db();
        settings_repo::set(&db, CACHE_KEY, &Backend::Cuda).unwrap();
        // detect_or_load should hit the cache and never call probe()
        let got = detect_or_load(&db);
        assert_eq!(got, Backend::Cuda);
    }

    #[test]
    fn probe_result_is_cached() {
        let db = mem_db();
        // First call probes (result depends on host) and caches.
        let first = detect_or_load(&db);
        let cached: Option<Backend> = settings_repo::get(&db, CACHE_KEY).unwrap();
        assert_eq!(cached, Some(first));
    }
}
