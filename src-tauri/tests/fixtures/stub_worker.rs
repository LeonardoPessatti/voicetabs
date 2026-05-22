//! Tiny stub worker for the supervisor integration test. Implements the same
//! framing as `stt_worker` but:
//!  - emits a fixed ReadyMessage,
//!  - replies to each request with a constant text "stub-ok",
//!  - if `STUB_DIE_AFTER` is set to N, exits with code 137 after N successful
//!    responses — used to simulate worker death.
//!  - if `STUB_DIE_MARKER` is set to a path, the stub touches that file at the
//!    moment it would die. On subsequent spawns, the stub checks for the marker
//!    and *skips* the die behavior, so the supervisor's respawn can be tested
//!    deterministically without the test process racing to clear an env var.

use std::env;
use std::io::{stdin, stdout, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const READY_JSON: &str = r#"{"ready":true,"model_id":"stub","backend":"stub"}"#;

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

fn write_frame<W: Write>(w: &mut W, body: &[u8]) {
    let len = body.len() as u32;
    w.write_all(&len.to_le_bytes()).unwrap();
    w.write_all(body).unwrap();
    w.flush().unwrap();
}

fn main() -> ExitCode {
    let marker: Option<PathBuf> = env::var("STUB_DIE_MARKER").ok().map(PathBuf::from);
    // If the marker exists, a previous incarnation already died once. Skip the
    // die behavior so the respawned child keeps serving requests.
    let already_died = marker
        .as_ref()
        .map(|p| p.exists())
        .unwrap_or(false);

    let die_after: Option<usize> = if already_died {
        None
    } else {
        env::var("STUB_DIE_AFTER").ok().and_then(|s| s.parse().ok())
    };

    let mut out = stdout().lock();
    write_frame(&mut out, READY_JSON.as_bytes());
    drop(out);

    let mut input = BufReader::new(stdin().lock());
    let mut count = 0usize;
    loop {
        let header = match read_frame(&mut input) {
            Some(h) => h,
            None => return ExitCode::SUCCESS,
        };
        let _pcm = match read_frame(&mut input) {
            Some(p) => p,
            None => return ExitCode::SUCCESS,
        };

        // Parse only the request_id field; we don't need the rest.
        let header_str = String::from_utf8_lossy(&header);
        let request_id = extract_request_id(&header_str).unwrap_or_else(|| "unknown".into());

        let resp = format!(
            r#"{{"request_id":"{}","text":"stub-ok","avg_logprob":-0.1,"no_speech_prob":0.05,"duration_ms":1}}"#,
            request_id
        );
        let mut out = stdout().lock();
        write_frame(&mut out, resp.as_bytes());
        drop(out);

        count += 1;
        if let Some(n) = die_after {
            if count >= n {
                // Touch the marker so the next spawn knows to skip dying.
                if let Some(ref m) = marker {
                    let _ = std::fs::write(m, b"died");
                }
                std::process::exit(137);
            }
        }
    }
}

fn extract_request_id(s: &str) -> Option<String> {
    // Minimal hand-rolled extraction so the stub has zero deps.
    let key = "\"request_id\":\"";
    let start = s.find(key)? + key.len();
    let rest = &s[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
