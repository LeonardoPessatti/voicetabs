# VoiceTabs — Phase 3 (STT Subprocess) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a separate `stt_worker` Rust binary that hosts `whisper.cpp` via `whisper-rs`, communicates with the main process over stdio using a length-prefixed framed protocol, and produces transcribed text for each utterance WAV that the existing Phase 2 capture pipeline emits. Two builds (`stt_worker_cuda.exe` and `stt_worker_cpu.exe`) ship in the installer; the main process probes the GPU at launch and picks one. The Whisper model file is bundled as a Tauri resource (no first-run download). At the end of Phase 3 the system logs the transcription text for every utterance and exposes a `stt-transcription` Tauri event the frontend can subscribe to — wiring the text to tabs and persisting segments is **deferred to Phase 4**.

**Architecture:** Phase 3 introduces a sibling crate `stt_worker` to the existing `src-tauri` member of the Cargo workspace. The worker is a thin command-line program: it reads frames from stdin, runs whisper.cpp inference, writes a response frame to stdout. The main process owns an `SttClient` + `SttSupervisor` pair: `SttClient` writes requests and reads responses over the child's stdio; `SttSupervisor` is a Tokio task that monitors the child, restarts it on death within 500 ms, and replays one in-flight utterance. The existing Phase 2 `CaptureController` gains a callback hook so that when `UtteranceBuilder` finalizes a WAV, the controller reads the f32 samples back from the WAV (cheapest path that preserves the existing builder API) and submits them to the `SttClient`. The model file `ggml-large-v3-turbo-q5_0.bin` is bundled into the installer via `tauri.conf.json > bundle.resources` and resolved at runtime through Tauri's `path::resolve_resource()` API. GPU autodetection runs once on first launch (`nvidia-smi`, fallback `wmic`, override via `WHISPER_CUDA` env var) and is cached in the `settings` table.

**Tech Stack additions:**
- `whisper-rs = "0.13"` — Rust bindings to whisper.cpp.
- `uuid = { version = "1", features = ["v4"] }` — request IDs.
- `tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "process", "io-util", "time"] }` — already present; we add `process`, `io-util`, `time`.
- `hound = "3.5"` — already present; reused to read back utterance WAVs.

**Reference spec:** `docs/superpowers/specs/2026-05-20-voicetabs-design.md` §4 (locked tech), §5.2 (dataflow), §5.3 (supervisor), §7.4 (IPC protocol), §7.5 (hallucination filter — Phase 4), §7.8 (status indicator), §7.9 (first-run wizard — **skipped in this phase per architecture decisions below**), §9 (packaging), §10 (phasing).

**Builds on:** Phase 0+1 plan (`docs/superpowers/plans/2026-05-20-voicetabs-foundation.md`) and Phase 2 plan (`docs/superpowers/plans/2026-05-20-voicetabs-phase-2-audio-vad.md`). Starting state: HEAD `cbcca06` on `master` branch, CI green, 37 Rust tests + 13 TS tests passing.

---

## Architecture decisions LOCKED for this phase

These decisions are firm and the plan below implements them as-is. They diverge from a literal reading of spec §7.9 in places; the divergences are intentional and called out where they bite.

1. **No first-run wizard.** Spec §7.9 describes a 7-step wizard (welcome, mic check, GPU detect, model download, benchmark, hotkey, done). We skip the wizard entirely. The wizard's purpose is obviated by bundling the model and silently probing the GPU. Mic-check + hotkey-binding move to Phase 5 (capture modes + hotkeys). Benchmark moves to Phase 7 (supervisor polish). If the GPU probe fails or the model fails to load, we surface a discreet yellow status dot in the footer (Task 17) — not a modal.
2. **Model is BUNDLED, not downloaded.** `ggml-large-v3-turbo-q5_0.bin` (~590 MB) ships in `src-tauri/resources/` and is referenced from `tauri.conf.json > bundle.resources`. The file is **not** committed to git (`.gitignore` rule added in Task 1). The engineer downloads it locally with a PowerShell snippet and verifies SHA-256. At runtime the main process calls `app.path().resolve_resource("ggml-large-v3-turbo-q5_0.bin")` to obtain the on-disk path, which it passes to `stt_worker` via `--model <path>`. Installer size goes from ~280 MB to ~870 MB.
3. **GPU autodetect at launch, cached in `settings`.**
   - First: shell out `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`. If the command exits zero and at least one row reports ≥ 4096 MiB, we pick CUDA.
   - Fallback if `nvidia-smi` is not on PATH: shell out `wmic path win32_VideoController get name /format:list` and look for the substring `NVIDIA` (case-insensitive). If found, optimistically pick CUDA; if the worker handshake then fails we fall back to CPU at runtime.
   - Override: if env var `WHISPER_CUDA` is set to `0` we force CPU; if set to `1` we force CUDA.
   - The decision is cached in the `settings` row keyed `stt_backend` (`"cuda"` / `"cpu"`). On subsequent launches we read the cached value and skip the probe. A future settings option to clear the cache is deferred.
4. **Two `stt_worker` binaries.** `stt_worker_cuda.exe` and `stt_worker_cpu.exe`. They are produced by `cargo build` invocations with different `--features` flags. Both ship in the installer via `tauri.conf.json > bundle.externalBin`.
5. **`whisper-rs` 0.13 with the `cuda` feature flag for the GPU build.** Default features only for the CPU build. We link statically against bundled `whisper.cpp` (the crate vendors it). On Windows the CUDA build also depends on `cudart64_12.dll` and `cublas64_12.dll`; the plan ships them via `bundle.resources` in Task 16 if they are not already discoverable.
6. **IPC matches spec §7.4 verbatim.** `<u32 LE length><bytes>` framing. JSON headers for handshake / request / response; raw f32 PCM for audio payload. Worker emits `{"ready": true, "model_id": "...", "backend": "cuda"|"cpu"}` on startup. Request: `{"request_id": "<uuid>", "sample_rate": 16000, "language": "pt", "initial_prompt": "<vocab>", "n_samples": <u32>}` + one PCM frame. Response: `{"request_id": "...", "text": "...", "avg_logprob": -0.3, "no_speech_prob": 0.02, "duration_ms": 450}` or `{"request_id": "...", "error": "..."}`.
7. **Supervisor: basic restart-on-death.** Per spec §5.3, simplified: Tokio task awaits child exit, on death respawns within 500 ms, replays a single in-flight utterance once, drops anything older than ~3 pending. **Hot-spare pattern (§5.3) is deferred to Phase 7.**
8. **`stt_worker` does NOT use Tokio.** It uses standard sync stdio (`std::io::stdin().lock()`, `std::io::stdout().lock()`). The main process is async on the Tauri/Tokio runtime; only the IPC bridge in main is async. Worker stays tiny and dependency-light.
9. **Phase 3 does NOT touch the `segments` table or render text on tabs.** The transcription text is (a) logged at `info` level in `voicetabs.log`, and (b) emitted as a Tauri event `stt-transcription` with `{request_id, text, started_at_ms, ended_at_ms, audio_path}` so dev-mode can inspect it in the WebView console. Wiring into segments + tab routing is Phase 4.

---

## Acceptance for this plan

- `stt_worker` builds in two flavors: `cargo build --release -p stt_worker --features cuda` produces `target/release/stt_worker_cuda.exe`; `cargo build --release -p stt_worker` (no features) produces `target/release/stt_worker_cpu.exe`. Both run standalone and print the handshake frame on startup when given `--model <path>`.
- Bundling: `npm run tauri build` produces an NSIS installer that includes both worker executables and the bundled `ggml-large-v3-turbo-q5_0.bin`.
- IPC round-trip: an integration test in `stt_worker/tests/round_trip.rs` spawns the worker as a subprocess, sends one PCM frame of a 3-5 s speech WAV, and receives a non-empty `text` field. (If no speech sample is available the test falls back to a 1 kHz sine and asserts only that the response framing parses correctly — see Task 15.)
- Supervisor: an integration test in `src-tauri/tests/stt_supervisor_test.rs` spawns a stub worker, kills the child mid-flight, and asserts the supervisor restarts + replays the in-flight request without surfacing an error to the caller.
- Frontend: a small yellow dot indicator appears in the footer when `stt_status` ≠ `ready`; green when `ready`. New `stt_status` Tauri command returns `{state: "loading" | "ready" | "restarting" | "error", backend: "cuda" | "cpu" | null}`.
- Manual: launch the app, click capture, speak one short PT-BR sentence, see the transcription text in `%APPDATA%\voicetabs\logs\voicetabs.log` and in the WebView dev console (the `stt-transcription` event payload). No segment row, no tab text — that's Phase 4.
- All prior unit + TS tests stay green.
- CI on master stays green.

## Out of scope for this plan

- Segment row insertion, segment UI cards, tab routing of transcribed text (all Phase 4).
- Hallucination filter (Phase 4 — it consumes the response fields this phase produces).
- First-run wizard, mic check UI, benchmark step (skipped permanently for the wizard; benchmark may resurface in Phase 7 if hot-spare is needed).
- Push-to-talk hotkey, capture-mode toggle, tray icon (Phase 5).
- Model selector / change-model UI / model download path (skipped; model is bundled). A future "switch model" feature can replace the bundled model with a downloaded one but is out of scope.
- Hot-spare worker pattern (Phase 7).
- Custom-vocabulary `initial_prompt` content. We send the prompt field with an empty string `""` for now; Phase 4 will populate it from `settings.vocab_terms`.

---

## File structure after this plan

```
transcript-tabs/
├── Cargo.toml                                # MODIFIED: add "stt_worker" to workspace members
├── .gitignore                                # MODIFIED: add src-tauri/resources/*.bin
├── stt_worker/                               # NEW workspace member
│   ├── Cargo.toml
│   ├── build.rs                              # rename built binary based on feature
│   ├── src/
│   │   ├── main.rs                           # entrypoint, arg parsing
│   │   ├── framing.rs                        # u32-LE framed read/write helpers (TDD)
│   │   ├── protocol.rs                       # request/response JSON structs
│   │   ├── whisper.rs                        # whisper-rs wrapper
│   │   └── lib.rs                            # re-exports for tests
│   └── tests/
│       └── round_trip.rs                     # subprocess-based integration test
├── src-tauri/
│   ├── Cargo.toml                            # MODIFIED: + uuid, tokio process/io-util/time
│   ├── tauri.conf.json                       # MODIFIED: + bundle.externalBin, bundle.resources
│   ├── resources/                            # NEW (NOT in git)
│   │   └── ggml-large-v3-turbo-q5_0.bin      # downloaded locally
│   ├── src/
│   │   ├── stt/                              # NEW
│   │   │   ├── mod.rs
│   │   │   ├── framing.rs                    # mirror of stt_worker/src/framing.rs (TDD)
│   │   │   ├── protocol.rs                   # request/response JSON structs
│   │   │   ├── gpu.rs                        # nvidia-smi / wmic probe + cache
│   │   │   ├── client.rs                     # SttClient (writes req, reads resp)
│   │   │   ├── supervisor.rs                 # SttSupervisor (Tokio task)
│   │   │   └── status.rs                     # shared SttStatus + handle
│   │   ├── commands/
│   │   │   ├── stt.rs                        # NEW: stt_status command
│   │   │   └── mod.rs                        # MODIFIED: declare stt
│   │   ├── capture/
│   │   │   └── controller.rs                 # MODIFIED: utterance-finalized callback
│   │   ├── paths.rs                          # MODIFIED: + model_resource_path()
│   │   └── lib.rs                            # MODIFIED: register supervisor + commands
│   └── tests/
│       ├── stt_framing_test.rs               # NEW
│       └── stt_supervisor_test.rs            # NEW
└── src/
    ├── lib/tauri.ts                          # MODIFIED: + sttApi + Tauri event listener type
    ├── stores/sttStore.ts                    # NEW
    ├── components/SttStatusDot.tsx           # NEW
    ├── i18n/locales/{pt-BR,en}.json          # MODIFIED: + stt.* keys
    ├── App.tsx                               # MODIFIED: mount status dot
    └── __tests__/
        ├── SttStatusDot.test.tsx             # NEW
        └── i18n.test.tsx                     # MODIFIED: mock stt_status
```

Each piece has one job:
- `stt_worker` is the inference subprocess. Sync stdio. Owns one `whisper-rs` context.
- `src-tauri/src/stt/` is the main-process side: framing utilities, JSON types, GPU probe, supervisor, client.
- `commands/stt.rs` exposes `stt_status` to the frontend; emitting `stt-transcription` is done from the supervisor.
- The frontend gets a small status dot and a typed event listener; rendering segments is Phase 4.

---

# Phase 3 tasks

## Task 1: Project setup — workspace + resources directory + .gitignore

Add the new workspace member, create the resources directory, and tell git to ignore bundled model binaries.

**Files:**
- Modify: `Cargo.toml`
- Modify: `.gitignore`
- Create: `src-tauri/resources/.gitkeep`

- [ ] **Step 1: Update the workspace `Cargo.toml`**

Replace the existing root `Cargo.toml` content with:

```toml
[workspace]
resolver = "2"
members = ["src-tauri", "stt_worker"]

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["VoiceTabs"]
license = "MIT"

[profile.release]
opt-level = 3
lto = "thin"
strip = "symbols"
```

- [ ] **Step 2: Update `.gitignore`**

Append to `.gitignore`:

```gitignore

# Bundled Whisper models — not committed; downloaded locally per README.
/src-tauri/resources/*.bin
```

- [ ] **Step 3: Create the resources directory placeholder**

```powershell
New-Item -ItemType Directory -Force -Path src-tauri\resources | Out-Null
New-Item -ItemType File -Force -Path src-tauri\resources\.gitkeep | Out-Null
```

- [ ] **Step 4: Commit**

```powershell
git add Cargo.toml .gitignore src-tauri/resources/.gitkeep
git commit -m "chore(workspace): add stt_worker member + resources dir"
```

---

## Task 2: Download the Whisper model + verify SHA-256

The Phase 3 model is `ggml-large-v3-turbo-q5_0.bin` from HuggingFace (`ggerganov/whisper.cpp`). The download is a one-time local action — the file is **not** committed to git.

**Files:**
- Create: `src-tauri/resources/ggml-large-v3-turbo-q5_0.bin` (local only)

- [x] **Step 1: Download the model**

Run this PowerShell snippet from the repo root. It uses `Invoke-WebRequest` with progress display; the file is ~547 MB.

```powershell
$Url    = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin"
$Out    = "src-tauri\resources\ggml-large-v3-turbo-q5_0.bin"
$Expect = "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"   # verified 2026-05-22
if (-not (Test-Path $Out)) {
    Write-Host "Downloading $Url (~547 MB)..."
    $ProgressPreference = "SilentlyContinue"  # speeds up Invoke-WebRequest
    Invoke-WebRequest -Uri $Url -OutFile $Out -UseBasicParsing
    $ProgressPreference = "Continue"
}
$Got = (Get-FileHash -Algorithm SHA256 $Out).Hash.ToLowerInvariant()
Write-Host "SHA-256: $Got"
if ($Got -ne $Expect) {
    Write-Warning "SHA-256 mismatch.`n  Expected: $Expect`n  Got     : $Got"
    Write-Warning "If you trust the source, update `$Expect in this script to the value above and re-run."
}
```

> **Note:** The expected SHA-256 above was verified against the canonical `huggingface.co/ggerganov/whisper.cpp` URL on 2026-05-22 (file size 574,041,195 bytes / ~547 MB). Future contributors can re-run this same command and the script will be silent on a clean download. **Do not** trust a third-party mirror; only use the canonical URL above.

- [x] **Step 2: Confirm the file exists and the size is plausible**

```powershell
$f = Get-Item src-tauri\resources\ggml-large-v3-turbo-q5_0.bin
Write-Host ("Size: {0:N0} bytes ({1:N1} MB)" -f $f.Length, ($f.Length / 1MB))
if ($f.Length -lt 500MB) { throw "Model file looks truncated" }
```

Expected: ~547 MB (574,041,195 bytes).

- [x] **Step 3: Confirm git does not track it**

```powershell
git status --porcelain src-tauri/resources/
```

Expected output: empty (or only `.gitkeep` if Task 1 wasn't committed yet). The `.bin` file must NOT appear.

- [x] **Step 4: No commit — this is a local file only.** (Plan doc updated separately to record the verified SHA-256 and actual size.)

---

## Task 3: Add Phase 3 dependencies to the main crate

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Update `[dependencies]` in `src-tauri/Cargo.toml`**

Add `uuid` and expand the `tokio` feature set. The full `[dependencies]` block after this task:

```toml
[dependencies]
anyhow = "1"
cpal = "0.15"
crossbeam-channel = "0.5"
directories = "5"
hound = "3.5"
ort = { version = "=2.0.0-rc.10", default-features = false, features = ["ndarray", "download-binaries"] }
parking_lot = "0.12"
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2.0", features = [] }
thiserror = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "process", "io-util", "time"] }
tracing = "0.1"
tracing-appender = "0.2"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
uuid = { version = "1", features = ["v4"] }
voice_activity_detector = "0.2.1"
```

- [ ] **Step 2: Run `cargo check`**

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: success.

- [ ] **Step 3: Re-run the existing test suite to confirm nothing regressed**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: 37 passed (Phase 2 baseline).

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/Cargo.toml Cargo.lock
git commit -m "chore(deps): add uuid + tokio process features for STT IPC"
```

---

## Task 4: Create the `stt_worker` crate skeleton

**Files:**
- Create: `stt_worker/Cargo.toml`
- Create: `stt_worker/src/main.rs`
- Create: `stt_worker/src/lib.rs`
- Create: `stt_worker/build.rs`

- [ ] **Step 1: Write `stt_worker/Cargo.toml`**

```toml
[package]
name = "stt_worker"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
description = "VoiceTabs STT worker — whisper.cpp subprocess"

[lib]
name = "stt_worker"
path = "src/lib.rs"

[[bin]]
name = "stt_worker_cpu"
path = "src/main.rs"
required-features = []

[[bin]]
name = "stt_worker_cuda"
path = "src/main.rs"
required-features = ["cuda"]

[features]
default = []
cuda = ["whisper-rs/cuda"]

[dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
whisper-rs = { version = "0.13", default-features = false }

[dev-dependencies]
hound = "3.5"
tempfile = "3"
```

Notes:
- Both `[[bin]]` entries point to the same `src/main.rs`. `cargo build -p stt_worker` produces `stt_worker_cpu.exe`; `cargo build -p stt_worker --features cuda` produces `stt_worker_cuda.exe` (and, because `required-features = []` is satisfied, also `stt_worker_cpu.exe` linked against the CUDA build of whisper-rs — which is fine but wasteful; the bundling step in Task 16 only ships the binary appropriate for each feature build).
- `whisper-rs = "0.13"` with `default-features = false` gets us the bare whisper.cpp build; the `cuda` feature flips on the CUDA backend. If `0.13` is not yet on crates.io at the time of execution, try `0.12` and report the version pin.
- We deliberately do NOT depend on `tokio` here. The worker is sync.

- [ ] **Step 2: Write a stub `stt_worker/src/lib.rs`**

```rust
//! Library surface for the STT worker. Modules here are re-exported so
//! integration tests under `stt_worker/tests/` can import them without going
//! through the binary.

pub mod framing;
pub mod protocol;
pub mod whisper;
```

(`framing` and `protocol` are added in the next tasks; `whisper` in Task 7. The build will fail until then; that's acceptable as we're staging.)

- [ ] **Step 3: Write a stub `stt_worker/src/main.rs`**

```rust
//! `stt_worker` entrypoint. Real implementation lands in Task 8.

fn main() {
    eprintln!("stt_worker placeholder — real entrypoint comes in Task 8");
    std::process::exit(2);
}
```

- [ ] **Step 4: Write `stt_worker/build.rs`** (empty for now; we may need it later if `whisper-rs` requires build env vars).

```rust
fn main() {
    // No build steps for now. whisper-rs handles its own native build.
}
```

- [ ] **Step 5: `cargo check` will fail because of the missing modules — that's expected. Commit the skeleton anyway.**

```powershell
git add stt_worker Cargo.toml
git commit -m "feat(stt_worker): crate skeleton (binaries + features)"
```

---

## Task 5: Framing utilities (TDD) — worker side

The IPC framing is `<u32 LE length><bytes>`. We implement read + write helpers, with unit tests over a `Cursor<Vec<u8>>`.

**Files:**
- Create: `stt_worker/src/framing.rs`

- [ ] **Step 1: Write `stt_worker/src/framing.rs` with tests inline**

```rust
//! Length-prefixed framing for the STT worker IPC.
//!
//! Wire format: `<u32 little-endian length><payload>` where payload is either
//! UTF-8 JSON or raw `f32` PCM. The two cases are disambiguated by context
//! (the worker alternates between "expecting JSON request" and "expecting PCM
//! payload" based on the request header it just parsed).

use std::io::{Read, Result as IoResult, Write};

/// Hard upper bound to refuse pathological frames. 256 MiB covers a 30 s
/// utterance at 16 kHz f32 (~1.92 MiB) with comfortable headroom.
pub const MAX_FRAME_BYTES: u32 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame length {0} exceeds MAX_FRAME_BYTES")]
    TooLarge(u32),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Write one frame: 4-byte LE length prefix + payload bytes.
pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> Result<(), FrameError> {
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

/// Read one frame's length prefix. Returns `None` on clean EOF (zero bytes
/// before the prefix). Returns an error on partial read.
pub fn read_frame_len<R: Read>(r: &mut R) -> Result<Option<u32>, FrameError> {
    let mut buf = [0u8; 4];
    let mut read = 0;
    while read < 4 {
        match r.read(&mut buf[read..])? {
            0 if read == 0 => return Ok(None),
            0 => {
                return Err(FrameError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "EOF mid-length-prefix",
                )))
            }
            n => read += n,
        }
    }
    let len = u32::from_le_bytes(buf);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    Ok(Some(len))
}

/// Read exactly `len` bytes (after the prefix). Used after a successful
/// `read_frame_len` call.
pub fn read_frame_body<R: Read>(r: &mut R, len: u32) -> IoResult<Vec<u8>> {
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Convenience: read one full frame (prefix + body). Returns `None` on clean
/// EOF before the prefix.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    match read_frame_len(r)? {
        None => Ok(None),
        Some(len) => Ok(Some(read_frame_body(r, len)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_small_payload() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        // 4-byte prefix + 5-byte body
        assert_eq!(buf.len(), 9);
        assert_eq!(&buf[0..4], &5u32.to_le_bytes());
        assert_eq!(&buf[4..], b"hello");

        let mut cur = Cursor::new(buf);
        let got = read_frame(&mut cur).unwrap().unwrap();
        assert_eq!(&got, b"hello");
    }

    #[test]
    fn round_trip_empty_payload() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"").unwrap();
        assert_eq!(buf, &[0u8, 0, 0, 0]);

        let mut cur = Cursor::new(buf);
        let got = read_frame(&mut cur).unwrap().unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn read_frame_returns_none_on_clean_eof() {
        let mut cur = Cursor::new(Vec::<u8>::new());
        let got = read_frame(&mut cur).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn read_frame_errors_on_partial_prefix() {
        let buf = vec![1u8, 0, 0]; // 3 bytes — incomplete prefix
        let mut cur = Cursor::new(buf);
        let err = read_frame(&mut cur).unwrap_err();
        assert!(matches!(err, FrameError::Io(_)));
    }

    #[test]
    fn read_frame_errors_when_body_truncated() {
        let mut buf = Vec::new();
        // Prefix says 5 bytes, only 2 follow.
        buf.extend_from_slice(&5u32.to_le_bytes());
        buf.extend_from_slice(b"hi");
        let mut cur = Cursor::new(buf);
        let err = read_frame(&mut cur).unwrap_err();
        assert!(matches!(err, FrameError::Io(_)));
    }

    #[test]
    fn write_frame_rejects_oversize() {
        // Simulate write of a payload larger than MAX_FRAME_BYTES.
        // We can't actually allocate 256 MiB in a unit test, so we test the
        // bound by hand-poking the limit via a custom slice of length MAX+1
        // — instead, just verify the constant has the expected value and the
        // type system would refuse u32::MAX + 1. The hard-coded check is in
        // read_frame_len; we test that path instead.
        let mut buf = Vec::new();
        let too_big = MAX_FRAME_BYTES + 1;
        buf.extend_from_slice(&too_big.to_le_bytes());
        let mut cur = Cursor::new(buf);
        let err = read_frame_len(&mut cur).unwrap_err();
        assert!(matches!(err, FrameError::TooLarge(_)));
    }

    #[test]
    fn round_trip_pcm_like_payload() {
        // Verify a non-trivial binary payload (e.g. f32-encoded) survives.
        let mut payload = Vec::with_capacity(4 * 16);
        for i in 0..16i32 {
            payload.extend_from_slice(&(i as f32).to_le_bytes());
        }
        let mut buf = Vec::new();
        write_frame(&mut buf, &payload).unwrap();
        let mut cur = Cursor::new(buf);
        let got = read_frame(&mut cur).unwrap().unwrap();
        assert_eq!(got, payload);
    }
}
```

- [ ] **Step 2: Run the framing tests**

```powershell
cargo test -p stt_worker --lib framing
```

Expected: 7 passed.

- [ ] **Step 3: Commit**

```powershell
git add stt_worker/src/framing.rs
git commit -m "feat(stt_worker): u32-LE framing read/write helpers"
```

---

## Task 6: Protocol JSON types — worker side

These structs match spec §7.4 verbatim and are shared (re-implemented identically) on the main-process side in Task 11.

**Files:**
- Create: `stt_worker/src/protocol.rs`

- [ ] **Step 1: Write `stt_worker/src/protocol.rs`**

```rust
//! IPC JSON message types. Matches `docs/superpowers/specs/2026-05-20-voicetabs-design.md` §7.4.

use serde::{Deserialize, Serialize};

/// Sent by the worker once on startup, after the model loads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyMessage {
    pub ready: bool,
    pub model_id: String,
    pub backend: String, // "cuda" | "cpu"
}

/// Sent by the main process before each PCM payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHeader {
    pub request_id: String,
    pub sample_rate: u32,
    pub language: String,
    pub initial_prompt: String,
    pub n_samples: u32,
}

/// Sent by the worker after running inference. Exactly one of `text` or
/// `error` is populated. The fields are flat (not nested) to match the spec
/// example payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_logprob: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_speech_prob: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ResponseMessage {
    pub fn ok(
        request_id: String,
        text: String,
        avg_logprob: f32,
        no_speech_prob: f32,
        duration_ms: u64,
    ) -> Self {
        Self {
            request_id,
            text: Some(text),
            avg_logprob: Some(avg_logprob),
            no_speech_prob: Some(no_speech_prob),
            duration_ms: Some(duration_ms),
            error: None,
        }
    }

    pub fn err(request_id: String, error: String) -> Self {
        Self {
            request_id,
            text: None,
            avg_logprob: None,
            no_speech_prob: None,
            duration_ms: None,
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_message_round_trip() {
        let m = ReadyMessage {
            ready: true,
            model_id: "ggml-large-v3-turbo-q5_0".into(),
            backend: "cuda".into(),
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: ReadyMessage = serde_json::from_str(&s).unwrap();
        assert!(back.ready);
        assert_eq!(back.backend, "cuda");
    }

    #[test]
    fn request_header_round_trip() {
        let h = RequestHeader {
            request_id: "abc".into(),
            sample_rate: 16_000,
            language: "pt".into(),
            initial_prompt: "VoiceTabs".into(),
            n_samples: 32_000,
        };
        let s = serde_json::to_string(&h).unwrap();
        let back: RequestHeader = serde_json::from_str(&s).unwrap();
        assert_eq!(back.n_samples, 32_000);
    }

    #[test]
    fn response_ok_serializes_without_error_field() {
        let r = ResponseMessage::ok("req-1".into(), "hello".into(), -0.3, 0.02, 450);
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("\"error\""));
        assert!(s.contains("\"text\""));
    }

    #[test]
    fn response_err_serializes_without_text_field() {
        let r = ResponseMessage::err("req-2".into(), "boom".into());
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"error\""));
        assert!(!s.contains("\"text\""));
    }
}
```

- [ ] **Step 2: Run the protocol tests**

```powershell
cargo test -p stt_worker --lib protocol
```

Expected: 4 passed.

- [ ] **Step 3: Commit**

```powershell
git add stt_worker/src/protocol.rs
git commit -m "feat(stt_worker): IPC message types per spec §7.4"
```

---

## Task 7: `whisper-rs` wrapper

Encapsulate `whisper-rs` so the main loop in `stt_worker/src/main.rs` is small. The wrapper loads a model once and exposes a single `transcribe(samples: &[f32], language: &str, initial_prompt: &str) -> TranscriptionResult`.

**Files:**
- Create: `stt_worker/src/whisper.rs`

- [ ] **Step 1: Write `stt_worker/src/whisper.rs`**

```rust
//! Thin wrapper around `whisper-rs`. The whole crate is built once with
//! either the CPU or CUDA feature flag — the `Engine` doesn't care which.

use std::path::Path;
use std::time::Instant;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error("whisper context init failed: {0}")]
    Init(String),
    #[error("whisper state init failed: {0}")]
    State(String),
    #[error("whisper inference failed: {0}")]
    Inference(String),
}

/// The transcription result returned by `Engine::transcribe`.
///
/// `text` is the concatenation of segment texts. `avg_logprob` and
/// `no_speech_prob` are the mean across segments (whisper.cpp exposes them
/// per segment via `state.full_get_segment_*`).
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub duration_ms: u64,
}

pub struct Engine {
    ctx: WhisperContext,
    model_id: String,
    n_threads: i32,
}

impl Engine {
    /// `model_path` is the absolute path to the `ggml-*.bin` file. `n_threads`
    /// is honored for the CPU build; the CUDA build ignores it internally.
    pub fn load(model_path: &Path, n_threads: i32) -> Result<Self, WhisperError> {
        let cparams = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(
            model_path.to_string_lossy().as_ref(),
            cparams,
        )
        .map_err(|e| WhisperError::Init(format!("{e:?}")))?;
        let model_id = model_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Ok(Self { ctx, model_id, n_threads })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn transcribe(
        &mut self,
        samples: &[f32],
        language: &str,
        initial_prompt: &str,
    ) -> Result<TranscriptionResult, WhisperError> {
        let started = Instant::now();
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| WhisperError::State(format!("{e:?}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.n_threads);
        params.set_translate(false);
        params.set_language(Some(language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if !initial_prompt.is_empty() {
            params.set_initial_prompt(initial_prompt);
        }

        state
            .full(params, samples)
            .map_err(|e| WhisperError::Inference(format!("{e:?}")))?;

        let n_segments = state
            .full_n_segments()
            .map_err(|e| WhisperError::Inference(format!("{e:?}")))?;

        let mut text = String::new();
        let mut avg_logprob_sum = 0.0_f64;
        let mut no_speech_prob_sum = 0.0_f64;

        for i in 0..n_segments {
            let seg = state
                .full_get_segment_text(i)
                .map_err(|e| WhisperError::Inference(format!("{e:?}")))?;
            text.push_str(&seg);
            // Per-segment confidence APIs were renamed in whisper-rs 0.13; we
            // fall back to defaults if a particular getter isn't available so
            // the wrapper compiles against the closest 0.13.x.
            if let Ok(p) = state.full_get_segment_avg_logprob(i) {
                avg_logprob_sum += p as f64;
            }
            if let Ok(p) = state.full_get_segment_no_speech_prob(i) {
                no_speech_prob_sum += p as f64;
            }
        }

        let n = if n_segments > 0 { n_segments as f64 } else { 1.0 };
        let avg_logprob = (avg_logprob_sum / n) as f32;
        let no_speech_prob = (no_speech_prob_sum / n) as f32;
        let duration_ms = started.elapsed().as_millis() as u64;

        Ok(TranscriptionResult {
            text: text.trim().to_string(),
            avg_logprob,
            no_speech_prob,
            duration_ms,
        })
    }
}
```

> **Executor note**: `whisper-rs 0.13` is the target. If the executor finds a newer minor (0.14+) is released and the APIs above have moved, prefer pinning back to 0.13 with `= "0.13"` rather than chasing renames. The two confidence-getter calls (`full_get_segment_avg_logprob`, `full_get_segment_no_speech_prob`) are the most likely to drift; the surrounding code is stable across the 0.x line. If neither exists in the installed version, hard-code `avg_logprob = -1.0` and `no_speech_prob = 0.0` and document the limitation in a TODO comment for Phase 7 polish.

- [ ] **Step 2: Run the worker's `cargo check`**

```powershell
cargo check -p stt_worker
```

Expected: success. The first invocation will compile `whisper-rs` and its bundled whisper.cpp (several minutes cold). If the build fails with `LINK : fatal error LNK1181: cannot open input file 'cudart64_12.lib'` or similar, run the same `cargo check` without `--features cuda` first to confirm the CPU path builds; CUDA-specific build issues are addressed in Task 16.

- [ ] **Step 3: Commit**

```powershell
git add stt_worker/src/whisper.rs
git commit -m "feat(stt_worker): whisper-rs Engine wrapper"
```

---

## Task 8: `stt_worker` main loop

Wire the framing + protocol + Whisper engine together. The worker:
1. Parses `--model <path>` and optional `--language <lang>` / `--threads <N>`.
2. Loads the model.
3. Emits the `ReadyMessage` frame to stdout.
4. Loops: read header frame → parse JSON → read PCM frame (`n_samples * 4` bytes) → call `Engine::transcribe` → write response frame.
5. On stdin EOF, exits cleanly with code 0.
6. On any error during inference, writes a `ResponseMessage::err` frame and continues.
7. On any fatal IO error, exits with code 1.

**Files:**
- Modify: `stt_worker/src/main.rs`

- [ ] **Step 1: Replace `stt_worker/src/main.rs`**

```rust
//! `stt_worker` entrypoint.
//!
//! Two binaries are produced from this same source file: `stt_worker_cpu.exe`
//! (default features) and `stt_worker_cuda.exe` (`--features cuda`). The only
//! observable difference is the `backend` field in the `ReadyMessage`.

use std::io::{stdin, stdout, BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use stt_worker::framing::{read_frame, write_frame};
use stt_worker::protocol::{ReadyMessage, RequestHeader, ResponseMessage};
use stt_worker::whisper::Engine;

#[cfg(feature = "cuda")]
const BACKEND_NAME: &str = "cuda";
#[cfg(not(feature = "cuda"))]
const BACKEND_NAME: &str = "cpu";

#[derive(Debug)]
struct Args {
    model: PathBuf,
    language: String,
    threads: i32,
}

fn parse_args() -> Result<Args, String> {
    let mut model: Option<PathBuf> = None;
    let mut language = "pt".to_string();
    let mut threads = num_cpus_default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--model" => {
                model = Some(PathBuf::from(it.next().ok_or("--model needs a value")?));
            }
            "--language" => {
                language = it.next().ok_or("--language needs a value")?;
            }
            "--threads" => {
                let v = it.next().ok_or("--threads needs a value")?;
                threads = v.parse::<i32>().map_err(|e| e.to_string())?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        model: model.ok_or("--model is required")?,
        language,
        threads,
    })
}

/// `num_cpus` would add a dependency; this approximates "logical cores - 1"
/// using `std::thread::available_parallelism`, clamped to [1, 16].
fn num_cpus_default() -> i32 {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    (logical - 1).clamp(1, 16)
}

fn init_tracing() {
    // Worker logs go to stderr so they don't corrupt the framed stdout.
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("VOICETABS_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true);
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .try_init();
}

fn run() -> Result<(), String> {
    init_tracing();
    let args = parse_args()?;
    tracing::info!("loading model: {} (backend={})", args.model.display(), BACKEND_NAME);
    let mut engine = Engine::load(&args.model, args.threads).map_err(|e| e.to_string())?;
    tracing::info!("model loaded: {}", engine.model_id());

    let ready = ReadyMessage {
        ready: true,
        model_id: engine.model_id().to_string(),
        backend: BACKEND_NAME.to_string(),
    };
    let ready_json = serde_json::to_vec(&ready).map_err(|e| e.to_string())?;
    let mut out = stdout().lock();
    write_frame(&mut out, &ready_json).map_err(|e| e.to_string())?;
    drop(out);

    let mut stdin_lock = BufReader::new(stdin().lock());

    loop {
        // 1. Header frame (JSON).
        let header_bytes = match read_frame(&mut stdin_lock) {
            Ok(Some(b)) => b,
            Ok(None) => {
                tracing::info!("stdin EOF — exiting cleanly");
                return Ok(());
            }
            Err(e) => return Err(format!("read header frame: {e}")),
        };
        let header: RequestHeader = match serde_json::from_slice(&header_bytes) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("malformed header JSON: {e}");
                continue;
            }
        };

        // 2. PCM frame.
        let pcm_bytes = match read_frame(&mut stdin_lock) {
            Ok(Some(b)) => b,
            Ok(None) => return Err("stdin EOF mid-request".into()),
            Err(e) => return Err(format!("read pcm frame: {e}")),
        };

        let expected_bytes = (header.n_samples as usize) * std::mem::size_of::<f32>();
        if pcm_bytes.len() != expected_bytes {
            let resp = ResponseMessage::err(
                header.request_id,
                format!(
                    "pcm size mismatch: header n_samples={} expects {} bytes, got {}",
                    header.n_samples,
                    expected_bytes,
                    pcm_bytes.len()
                ),
            );
            write_response(&resp)?;
            continue;
        }

        let samples: Vec<f32> = pcm_bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        // 3. Inference.
        let resp = match engine.transcribe(&samples, &header.language, &header.initial_prompt) {
            Ok(r) => ResponseMessage::ok(
                header.request_id,
                r.text,
                r.avg_logprob,
                r.no_speech_prob,
                r.duration_ms,
            ),
            Err(e) => ResponseMessage::err(header.request_id, e.to_string()),
        };

        write_response(&resp)?;
    }
}

fn write_response(resp: &ResponseMessage) -> Result<(), String> {
    let json = serde_json::to_vec(resp).map_err(|e| e.to_string())?;
    let mut out = stdout().lock();
    write_frame(&mut out, &json).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("fatal: {e}");
            eprintln!("stt_worker: fatal: {e}");
            ExitCode::from(1)
        }
    }
}
```

- [ ] **Step 2: Build the CPU worker**

```powershell
cargo build -p stt_worker
```

Expected: success. Output binary: `target/debug/stt_worker_cpu.exe`. First build is slow (~5–10 min).

- [ ] **Step 3: Smoke-run the CPU worker by piping in an empty stream**

```powershell
$ModelPath = "src-tauri\resources\ggml-large-v3-turbo-q5_0.bin"
# Open stdin from $null so EOF arrives immediately; we expect the ready frame
# then a clean exit.
"" | & target\debug\stt_worker_cpu.exe --model $ModelPath 2>&1 | Out-Null
$LASTEXITCODE
```

Expected: prints model-load progress to stderr (which goes to the terminal here), then exits with code 0. If you see exit code 1 or a `whisper context init failed` message, the model path is wrong or the file is corrupt — verify Task 2.

- [ ] **Step 4: Commit**

```powershell
git add stt_worker/src/main.rs
git commit -m "feat(stt_worker): main loop with framed stdio + whisper inference"
```

---

## Task 9: Mirror framing utilities on the main-process side (TDD)

The main process needs the same `read_frame` / `write_frame` helpers. We can't share code from `stt_worker` because `src-tauri` doesn't depend on `stt_worker` as a library (and shouldn't — circular feature concerns). So we re-implement, with the same tests.

**Files:**
- Create: `src-tauri/src/stt/mod.rs`
- Create: `src-tauri/src/stt/framing.rs`
- Create: `src-tauri/tests/stt_framing_test.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod stt`)

- [ ] **Step 1: Create `src-tauri/src/stt/mod.rs`**

```rust
pub mod client;
pub mod framing;
pub mod gpu;
pub mod protocol;
pub mod status;
pub mod supervisor;

pub use status::{SttStatus, SttStatusHandle};
pub use supervisor::SttSupervisor;
```

(The other submodules land in later tasks; declare them now so the module tree is final.)

- [ ] **Step 2: Create `src-tauri/src/stt/framing.rs`**

Same code as `stt_worker/src/framing.rs` (Task 5), but instead of duplicating the test block we put a smaller smoke test in `src-tauri/tests/stt_framing_test.rs`. Copy the **non-test** portion of Task 5's `framing.rs` into this file. To be explicit:

```rust
//! Length-prefixed framing helpers on the main-process side. Mirrors
//! `stt_worker/src/framing.rs`. See spec §7.4.

use std::io::{Read, Result as IoResult, Write};

pub const MAX_FRAME_BYTES: u32 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame length {0} exceeds MAX_FRAME_BYTES")]
    TooLarge(u32),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> Result<(), FrameError> {
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

pub fn read_frame_len<R: Read>(r: &mut R) -> Result<Option<u32>, FrameError> {
    let mut buf = [0u8; 4];
    let mut read = 0;
    while read < 4 {
        match r.read(&mut buf[read..])? {
            0 if read == 0 => return Ok(None),
            0 => {
                return Err(FrameError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "EOF mid-length-prefix",
                )))
            }
            n => read += n,
        }
    }
    let len = u32::from_le_bytes(buf);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    Ok(Some(len))
}

pub fn read_frame_body<R: Read>(r: &mut R, len: u32) -> IoResult<Vec<u8>> {
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    match read_frame_len(r)? {
        None => Ok(None),
        Some(len) => Ok(Some(read_frame_body(r, len)?)),
    }
}

/// Tokio-async write of one frame. Used by `SttClient` to write to the child's
/// stdin without blocking the runtime.
pub async fn write_frame_async<W>(w: &mut W, payload: &[u8]) -> Result<(), FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes()).await?;
    w.write_all(payload).await?;
    w.flush().await?;
    Ok(())
}

/// Tokio-async read of one full frame. Returns `Ok(None)` on clean EOF.
pub async fn read_frame_async<R>(r: &mut R) -> Result<Option<Vec<u8>>, FrameError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4];
    match r.read_exact(&mut buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(FrameError::Io(e)),
    }
    let len = u32::from_le_bytes(buf);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(Some(body))
}
```

- [ ] **Step 3: Create `src-tauri/tests/stt_framing_test.rs`**

```rust
//! Smoke tests for the main-process side of the framing protocol. Mirrors the
//! worker-side tests in stt_worker/src/framing.rs.

use std::io::Cursor;

use voicetabs_lib::stt::framing::{read_frame, read_frame_async, write_frame, write_frame_async, FrameError};

#[test]
fn sync_round_trip() {
    let mut buf = Vec::new();
    write_frame(&mut buf, b"hello").unwrap();
    let mut cur = Cursor::new(buf);
    let got = read_frame(&mut cur).unwrap().unwrap();
    assert_eq!(got, b"hello");
}

#[test]
fn sync_eof_is_none() {
    let mut cur = Cursor::new(Vec::<u8>::new());
    assert!(read_frame(&mut cur).unwrap().is_none());
}

#[test]
fn sync_rejects_oversize_prefix() {
    let too_big = 257u32 * 1024 * 1024;
    let mut buf = Vec::new();
    buf.extend_from_slice(&too_big.to_le_bytes());
    let mut cur = Cursor::new(buf);
    match read_frame(&mut cur) {
        Err(FrameError::TooLarge(_)) => {}
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

#[tokio::test]
async fn async_round_trip() {
    use tokio::io::AsyncWriteExt;
    let (mut client, mut server) = tokio::io::duplex(64);
    // Spawn a writer task that writes one frame then closes the write half.
    let writer = tokio::spawn(async move {
        write_frame_async(&mut client, b"async-hello").await.unwrap();
        client.shutdown().await.unwrap();
    });
    let got = read_frame_async(&mut server).await.unwrap().unwrap();
    writer.await.unwrap();
    assert_eq!(got, b"async-hello");
}
```

> **Executor note**: this test uses `#[tokio::test]` which requires the `tokio-test` macro infrastructure already present via the `tokio` crate's `macros` feature (we added `macros` in Task 3). If the test won't compile due to missing macros, ensure `tokio = { features = [..., "macros", "rt"] }` includes `rt`. Our Phase 2 baseline already has `rt-multi-thread` which transitively enables `rt`.

- [ ] **Step 4: Declare `stt` module in `src-tauri/src/lib.rs`**

The module list becomes:

```rust
pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;
pub mod stt;
pub mod utterance;
pub mod vad;
```

(Other submodules — `client`, `gpu`, `protocol`, `status`, `supervisor` — are referenced in `mod.rs` already. Their files come in later tasks; for now the module tree won't compile. **Add empty stubs in the next sub-step so it does**.)

- [ ] **Step 5: Add empty stubs so the crate compiles after this task**

```powershell
@'
//! Placeholder — real implementation in Task 11.
'@ | Set-Content -Encoding utf8 src-tauri\src\stt\protocol.rs

@'
//! Placeholder — real implementation in Task 10.
'@ | Set-Content -Encoding utf8 src-tauri\src\stt\gpu.rs

@'
//! Placeholder — real implementation in Task 12.
'@ | Set-Content -Encoding utf8 src-tauri\src\stt\status.rs

@'
//! Placeholder — real implementation in Task 12.
pub struct SttSupervisor;
'@ | Set-Content -Encoding utf8 src-tauri\src\stt\supervisor.rs

@'
//! Placeholder — real implementation in Task 11.
'@ | Set-Content -Encoding utf8 src-tauri\src\stt\client.rs
```

Update `src-tauri/src/stt/mod.rs` to NOT re-export anything from those placeholder files yet — keep the `pub use ...` lines but they must reference items that actually exist. So change `mod.rs` to:

```rust
pub mod client;
pub mod framing;
pub mod gpu;
pub mod protocol;
pub mod status;
pub mod supervisor;

pub use supervisor::SttSupervisor;
```

(`SttStatus` and `SttStatusHandle` come back to the re-export list in Task 12.)

- [ ] **Step 6: Run the framing tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test stt_framing_test
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: 4 new tests pass; full suite 41 passed (37 baseline + 4 new).

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/stt src-tauri/src/lib.rs src-tauri/tests/stt_framing_test.rs
git commit -m "feat(stt): main-process framing helpers (sync + tokio async)"
```

---

## Task 10: GPU detection helper + cache

The decision tree:
1. If env var `WHISPER_CUDA` is set, use it (`"0"` → CPU, anything else → CUDA).
2. Otherwise, check the cached `stt_backend` setting; if present, use it.
3. Otherwise, probe `nvidia-smi`; if it returns at least one GPU with ≥ 4096 MiB, choose CUDA.
4. Otherwise, probe `wmic`; if any video controller name contains `NVIDIA` (case-insensitive), choose CUDA optimistically.
5. Otherwise, CPU.
6. Persist the choice in the `settings` row.

**Files:**
- Modify (replace placeholder): `src-tauri/src/stt/gpu.rs`

- [ ] **Step 1: Write `src-tauri/src/stt/gpu.rs`**

```rust
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
```

- [ ] **Step 2: Run the GPU tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml stt::gpu
```

Expected: 4 passed. Note: `probe_result_is_cached` runs `probe()`, which may shell out to `nvidia-smi` / `wmic`. Both are tolerant of missing binaries and return `false` cleanly. The test only checks that whatever the result is, it's cached.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/stt/gpu.rs
git commit -m "feat(stt): layered GPU autodetect with settings cache"
```

---

## Task 11: Protocol types + `SttClient`

`SttClient` owns the **write** side of the child's stdin and the **read** side of its stdout. It exposes a single `async transcribe(samples: Vec<f32>, vocab_prompt: String) -> Result<TranscriptionResult, SttClientError>` method. Routing responses to the right call uses a `oneshot::Sender` keyed by `request_id`, parked in a `HashMap` behind a `Mutex`. A dedicated Tokio task drains stdout frames and routes them.

**Files:**
- Modify (replace placeholder): `src-tauri/src/stt/protocol.rs`
- Modify (replace placeholder): `src-tauri/src/stt/client.rs`

- [ ] **Step 1: Write `src-tauri/src/stt/protocol.rs`**

(Same as `stt_worker/src/protocol.rs` from Task 6 — copy the entire file. We could share via a third workspace crate, but for two short files the duplication is cheaper.)

```rust
//! IPC message types. Mirrors `stt_worker/src/protocol.rs`. See spec §7.4.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyMessage {
    pub ready: bool,
    pub model_id: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHeader {
    pub request_id: String,
    pub sample_rate: u32,
    pub language: String,
    pub initial_prompt: String,
    pub n_samples: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_logprob: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_speech_prob: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The transcription result presented to the rest of the main process. This is
/// the canonical type used by `SttClient::transcribe`, the supervisor's
/// in-flight queue, and the `stt-transcription` Tauri event payload.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionResult {
    pub request_id: String,
    pub text: String,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub duration_ms: u64,
}

impl TranscriptionResult {
    pub fn from_response(r: ResponseMessage) -> Result<Self, String> {
        match (r.text, r.error) {
            (Some(text), None) => Ok(Self {
                request_id: r.request_id,
                text,
                avg_logprob: r.avg_logprob.unwrap_or(-1.0),
                no_speech_prob: r.no_speech_prob.unwrap_or(0.0),
                duration_ms: r.duration_ms.unwrap_or(0),
            }),
            (_, Some(err)) => Err(err),
            (None, None) => Err("response missing both text and error".into()),
        }
    }
}
```

- [ ] **Step 2: Write `src-tauri/src/stt/client.rs`**

```rust
//! `SttClient` — write requests to the worker's stdin, route stdout responses
//! back to the matching call.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{oneshot, Mutex as TokioMutex};

use crate::stt::framing::{read_frame_async, write_frame_async, FrameError};
use crate::stt::protocol::{
    ReadyMessage, RequestHeader, ResponseMessage, TranscriptionResult,
};

#[derive(Debug, thiserror::Error)]
pub enum SttClientError {
    #[error("worker channel closed before response")]
    Closed,
    #[error("worker returned error: {0}")]
    Worker(String),
    #[error("framing: {0}")]
    Framing(#[from] FrameError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

type ResponseTx = oneshot::Sender<Result<TranscriptionResult, SttClientError>>;

/// `SttClient` is `Clone` because all internal state is `Arc`-shared. Callers
/// can hand a clone to each utterance task.
#[derive(Clone)]
pub struct SttClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    stdin_tx: tokio::sync::mpsc::Sender<RequestEnvelope>,
    pending: Arc<Mutex<HashMap<String, ResponseTx>>>,
}

struct RequestEnvelope {
    header: RequestHeader,
    pcm: Vec<f32>,
}

impl SttClient {
    /// Construct the client and spawn the two background tasks (writer + reader).
    /// Returns once we've read the worker's `ReadyMessage`.
    pub async fn handshake<R, W>(
        mut stdout: R,
        stdin: W,
    ) -> Result<(Self, ReadyMessage), SttClientError>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        // 1. Read the ReadyMessage frame.
        let frame = read_frame_async(&mut stdout)
            .await?
            .ok_or(SttClientError::Closed)?;
        let ready: ReadyMessage = serde_json::from_slice(&frame)?;
        if !ready.ready {
            return Err(SttClientError::Worker("worker reported not ready".into()));
        }

        // 2. Spawn writer task: drains `stdin_rx`, writes header + PCM to stdin.
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<RequestEnvelope>(8);
        let stdin = Arc::new(TokioMutex::new(stdin));
        let stdin_for_writer = stdin.clone();
        tokio::spawn(async move {
            while let Some(env) = stdin_rx.recv().await {
                let mut w = stdin_for_writer.lock().await;
                let header_json = match serde_json::to_vec(&env.header) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!("serialize header: {e}");
                        continue;
                    }
                };
                if let Err(e) = write_frame_async(&mut *w, &header_json).await {
                    tracing::error!("write header frame: {e}");
                    break;
                }
                let mut pcm_bytes = Vec::with_capacity(env.pcm.len() * 4);
                for s in env.pcm {
                    pcm_bytes.extend_from_slice(&s.to_le_bytes());
                }
                if let Err(e) = write_frame_async(&mut *w, &pcm_bytes).await {
                    tracing::error!("write pcm frame: {e}");
                    break;
                }
            }
        });

        // 3. Spawn reader task: drains stdout, dispatches to pending map.
        let pending: Arc<Mutex<HashMap<String, ResponseTx>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_for_reader = pending.clone();
        tokio::spawn(async move {
            let mut stdout = stdout;
            loop {
                let frame = match read_frame_async(&mut stdout).await {
                    Ok(Some(f)) => f,
                    Ok(None) => {
                        tracing::info!("stt_worker stdout EOF");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("read response frame: {e}");
                        break;
                    }
                };
                let resp: ResponseMessage = match serde_json::from_slice(&frame) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("malformed response JSON: {e}");
                        continue;
                    }
                };
                let id = resp.request_id.clone();
                let tx = pending_for_reader.lock().remove(&id);
                if let Some(tx) = tx {
                    let outcome = match TranscriptionResult::from_response(resp) {
                        Ok(r) => Ok(r),
                        Err(e) => Err(SttClientError::Worker(e)),
                    };
                    let _ = tx.send(outcome);
                } else {
                    tracing::warn!("response for unknown request_id {id}");
                }
            }
            // Worker died — fail every pending request.
            let mut map = pending_for_reader.lock();
            for (_, tx) in map.drain() {
                let _ = tx.send(Err(SttClientError::Closed));
            }
        });

        Ok((
            SttClient {
                inner: Arc::new(ClientInner { stdin_tx, pending }),
            },
            ready,
        ))
    }

    /// Submit one utterance for transcription. The future resolves when the
    /// worker returns a response for this `request_id`, or fails if the worker
    /// dies in the meantime.
    pub async fn transcribe(
        &self,
        request_id: String,
        samples: Vec<f32>,
        language: &str,
        initial_prompt: &str,
    ) -> Result<TranscriptionResult, SttClientError> {
        let header = RequestHeader {
            request_id: request_id.clone(),
            sample_rate: 16_000,
            language: language.to_string(),
            initial_prompt: initial_prompt.to_string(),
            n_samples: samples.len() as u32,
        };
        let (resp_tx, resp_rx) = oneshot::channel();
        self.inner.pending.lock().insert(request_id, resp_tx);
        self.inner
            .stdin_tx
            .send(RequestEnvelope { header, pcm: samples })
            .await
            .map_err(|_| SttClientError::Closed)?;
        resp_rx.await.map_err(|_| SttClientError::Closed)?
    }
}
```

- [ ] **Step 3: Run a quick compile check**

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: clean.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/stt/protocol.rs src-tauri/src/stt/client.rs
git commit -m "feat(stt): SttClient + protocol types for request routing"
```

---

## Task 12: `SttStatus` + `SttSupervisor` (Tokio task)

The supervisor owns the lifetime of the child process. It spawns the worker, performs the handshake, exposes the `SttClient`, and on child exit respawns + replays the most recent in-flight utterance.

**Files:**
- Modify (replace placeholder): `src-tauri/src/stt/status.rs`
- Modify (replace placeholder): `src-tauri/src/stt/supervisor.rs`
- Modify: `src-tauri/src/stt/mod.rs`

- [ ] **Step 1: Write `src-tauri/src/stt/status.rs`**

```rust
//! Shared status published from the supervisor to the frontend via Tauri events
//! and to commands via a `manage()`-stored handle.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SttStatus {
    /// Worker process is spawning and the model is still loading.
    Loading { backend: String },
    /// Worker is ready; transcriptions can be issued.
    Ready {
        backend: String,
        model_id: String,
    },
    /// Worker died; supervisor is in the middle of respawning.
    Restarting { backend: String },
    /// A fatal error occurred (e.g. model file missing on disk).
    Error { message: String },
}

#[derive(Clone)]
pub struct SttStatusHandle {
    inner: Arc<Mutex<SttStatus>>,
}

impl SttStatusHandle {
    pub fn new(initial: SttStatus) -> Self {
        Self { inner: Arc::new(Mutex::new(initial)) }
    }

    pub fn get(&self) -> SttStatus {
        self.inner.lock().clone()
    }

    pub fn set(&self, s: SttStatus) {
        *self.inner.lock() = s;
    }
}
```

- [ ] **Step 2: Write `src-tauri/src/stt/supervisor.rs`**

```rust
//! `SttSupervisor` owns the worker child process.
//!
//! Responsibilities:
//! - Spawn the chosen worker binary with `--model <path>`.
//! - Run the IPC handshake via `SttClient::handshake`.
//! - Re-spawn on child exit within 500 ms; re-handshake; replay the last
//!   in-flight utterance once.
//! - Update `SttStatusHandle` and emit `stt-status` events on changes.
//! - Provide `transcribe()` for the rest of the main process to call.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::process::{Child, Command};
use tokio::sync::Mutex as TokioMutex;
use tokio::time::Duration;

use crate::stt::client::{SttClient, SttClientError};
use crate::stt::gpu::Backend;
use crate::stt::protocol::TranscriptionResult;
use crate::stt::status::{SttStatus, SttStatusHandle};

const RESPAWN_DELAY_MS: u64 = 500;
const MAX_REPLAY_ATTEMPTS: usize = 1;

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub worker_binary: PathBuf,
    pub model_path: PathBuf,
    pub language: String,
    pub backend: Backend,
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("failed to spawn worker: {0}")]
    Spawn(String),
    #[error("handshake failed: {0}")]
    Handshake(String),
    #[error("worker error: {0}")]
    Worker(String),
}

/// Public handle held inside Tauri's `manage()` slot.
#[derive(Clone)]
pub struct SttSupervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    cfg: SupervisorConfig,
    status: SttStatusHandle,
    client: TokioMutex<Option<SttClient>>,
    /// Most recent in-flight utterance (for replay on child death). Wrapped in
    /// a sync Mutex because callers may inspect it without awaiting.
    last_inflight: Mutex<Option<InFlight>>,
}

#[derive(Clone)]
struct InFlight {
    request_id: String,
    samples: Vec<f32>,
    initial_prompt: String,
    audio_path: Option<PathBuf>,
    started_at_ms: u64,
}

impl SttSupervisor {
    pub fn new(cfg: SupervisorConfig, status: SttStatusHandle) -> Self {
        Self {
            inner: Arc::new(SupervisorInner {
                cfg,
                status,
                client: TokioMutex::new(None),
                last_inflight: Mutex::new(None),
            }),
        }
    }

    pub fn status_handle(&self) -> SttStatusHandle {
        self.inner.status.clone()
    }

    /// Boot the worker for the first time. Call once at app startup. Errors
    /// surface as `SttStatus::Error` via the status handle; this function
    /// returns `Ok(())` even on spawn failure so the rest of the app can boot.
    pub async fn boot(&self) -> Result<(), SupervisorError> {
        self.inner.status.set(SttStatus::Loading {
            backend: self.inner.cfg.backend.as_str().to_string(),
        });
        match self.spawn_and_handshake().await {
            Ok((child, client, model_id)) => {
                *self.inner.client.lock().await = Some(client);
                self.inner.status.set(SttStatus::Ready {
                    backend: self.inner.cfg.backend.as_str().to_string(),
                    model_id,
                });
                // Spawn the watcher in the background.
                let me = self.clone();
                tokio::spawn(async move { me.watch_child(child).await });
                Ok(())
            }
            Err(e) => {
                self.inner.status.set(SttStatus::Error { message: e.to_string() });
                Err(e)
            }
        }
    }

    /// Public transcribe. Records `last_inflight` so we can replay on death.
    pub async fn transcribe(
        &self,
        request_id: String,
        samples: Vec<f32>,
        language: &str,
        initial_prompt: &str,
        audio_path: Option<PathBuf>,
        started_at_ms: u64,
    ) -> Result<TranscriptionResult, SupervisorError> {
        // Record before we await — even a spawn-failure replay needs it.
        *self.inner.last_inflight.lock() = Some(InFlight {
            request_id: request_id.clone(),
            samples: samples.clone(),
            initial_prompt: initial_prompt.to_string(),
            audio_path,
            started_at_ms,
        });

        let client = {
            let guard = self.inner.client.lock().await;
            guard
                .clone()
                .ok_or_else(|| SupervisorError::Worker("no client (worker down)".into()))?
        };

        match client.transcribe(request_id, samples, language, initial_prompt).await {
            Ok(r) => {
                self.inner.last_inflight.lock().take();
                Ok(r)
            }
            Err(SttClientError::Closed) => {
                // Worker died mid-flight; the watcher will respawn and replay.
                Err(SupervisorError::Worker("worker closed mid-request".into()))
            }
            Err(e) => Err(SupervisorError::Worker(e.to_string())),
        }
    }

    async fn spawn_and_handshake(
        &self,
    ) -> Result<(Child, SttClient, String), SupervisorError> {
        let mut cmd = Command::new(&self.inner.cfg.worker_binary);
        cmd.arg("--model")
            .arg(&self.inner.cfg.model_path)
            .arg("--language")
            .arg(&self.inner.cfg.language)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| SupervisorError::Spawn(e.to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SupervisorError::Spawn("no stdout".into()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SupervisorError::Spawn("no stdin".into()))?;
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            tokio::spawn(forward_stderr(stderr));
        }
        let (client, ready) = SttClient::handshake(stdout, stdin)
            .await
            .map_err(|e| SupervisorError::Handshake(e.to_string()))?;
        Ok((child, client, ready.model_id))
    }

    async fn watch_child(self, mut child: Child) {
        let status = child.wait().await;
        tracing::warn!("stt_worker child exited: {status:?}");

        // Update status, then attempt respawn + replay.
        self.inner.status.set(SttStatus::Restarting {
            backend: self.inner.cfg.backend.as_str().to_string(),
        });
        tokio::time::sleep(Duration::from_millis(RESPAWN_DELAY_MS)).await;

        match self.spawn_and_handshake().await {
            Ok((new_child, new_client, model_id)) => {
                *self.inner.client.lock().await = Some(new_client.clone());
                self.inner.status.set(SttStatus::Ready {
                    backend: self.inner.cfg.backend.as_str().to_string(),
                    model_id,
                });
                // Replay last in-flight (one attempt).
                let inflight = self.inner.last_inflight.lock().take();
                if let Some(inflight) = inflight {
                    tracing::info!("replaying in-flight utterance {}", inflight.request_id);
                    let _ = new_client
                        .transcribe(
                            inflight.request_id,
                            inflight.samples,
                            &self.inner.cfg.language,
                            &inflight.initial_prompt,
                        )
                        .await;
                }
                // Continue watching.
                let me = self.clone();
                tokio::spawn(async move { me.watch_child(new_child).await });
            }
            Err(e) => {
                tracing::error!("respawn failed: {e}");
                self.inner.status.set(SttStatus::Error {
                    message: format!("respawn failed: {e}"),
                });
                let _ = MAX_REPLAY_ATTEMPTS; // suppress unused-const warning
            }
        }
    }
}

async fn forward_stderr<R: tokio::io::AsyncRead + Unpin + Send + 'static>(stderr: R) {
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(stderr).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        tracing::info!(target: "stt_worker", "{}", line);
    }
}
```

- [ ] **Step 3: Update `src-tauri/src/stt/mod.rs`** to re-export the now-real types:

```rust
pub mod client;
pub mod framing;
pub mod gpu;
pub mod protocol;
pub mod status;
pub mod supervisor;

pub use status::{SttStatus, SttStatusHandle};
pub use supervisor::{SttSupervisor, SupervisorConfig, SupervisorError};
```

- [ ] **Step 4: `cargo check`**

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: clean compile.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/stt
git commit -m "feat(stt): supervisor with respawn + in-flight replay"
```

---

## Task 13: Supervisor restart drill (integration test)

We can't run a real `whisper-rs` worker in a unit test (the model is 590 MB and the inference is slow). Instead, build a **stub** worker as a tiny inline test fixture — a separate small Rust binary that obeys the framing protocol but echoes a fixed response — and exercise the supervisor's restart logic against it.

**Files:**
- Create: `src-tauri/tests/fixtures/stub_worker.rs`
- Create: `src-tauri/tests/stt_supervisor_test.rs`
- Modify: `src-tauri/Cargo.toml` (declare the stub bin)

- [ ] **Step 1: Declare the stub binary in `src-tauri/Cargo.toml`**

Append to `src-tauri/Cargo.toml`:

```toml
[[bin]]
name = "stub_worker"
path = "tests/fixtures/stub_worker.rs"
required-features = []
```

The bin only compiles during tests. We mark it `path = ...` instead of putting it under `examples/` so `cargo test` picks it up via `--bin stub_worker`.

- [ ] **Step 2: Write `src-tauri/tests/fixtures/stub_worker.rs`**

```rust
//! Tiny stub worker for the supervisor integration test. Implements the same
//! framing as `stt_worker` but:
//!  - emits a fixed ReadyMessage,
//!  - replies to each request with a constant text "stub-ok",
//!  - if the env var `STUB_DIE_AFTER` is set to N, exits with code 137 after
//!    N successful responses — used to simulate worker death.

use std::env;
use std::io::{stdin, stdout, BufReader, Read, Write};
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
    let die_after: Option<usize> = env::var("STUB_DIE_AFTER")
        .ok()
        .and_then(|s| s.parse().ok());

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
```

- [ ] **Step 3: Write `src-tauri/tests/stt_supervisor_test.rs`**

```rust
//! Integration test: spin up the stub worker, exercise the supervisor's happy
//! path + restart path.
//!
//! These tests intentionally do NOT exercise whisper-rs. The full WAV-to-text
//! round trip lives in `stt_worker/tests/round_trip.rs` (Task 15).

use std::path::PathBuf;

use voicetabs_lib::stt::gpu::Backend;
use voicetabs_lib::stt::status::SttStatus;
use voicetabs_lib::stt::{SttStatusHandle, SttSupervisor, SupervisorConfig};

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

fn make_cfg(env: &[(&str, &str)]) -> SupervisorConfig {
    for (k, v) in env {
        std::env::set_var(k, v);
    }
    SupervisorConfig {
        worker_binary: stub_path(),
        model_path: PathBuf::from("unused"), // stub ignores --model
        language: "pt".into(),
        backend: Backend::Cpu,
    }
}

#[tokio::test]
async fn happy_path_transcribes_once() {
    let cfg = make_cfg(&[]);
    let status = SttStatusHandle::new(SttStatus::Loading { backend: "cpu".into() });
    let sup = SttSupervisor::new(cfg, status);
    sup.boot().await.expect("boot ok");

    let result = sup
        .transcribe(
            "req-1".into(),
            vec![0.0_f32; 256],
            "pt",
            "",
            None,
            0,
        )
        .await
        .expect("transcribe ok");
    assert_eq!(result.request_id, "req-1");
    assert_eq!(result.text, "stub-ok");
}

#[tokio::test]
async fn worker_death_is_recovered_within_two_seconds() {
    // Stub dies after one successful response. Supervisor must respawn and a
    // second transcribe call (issued after the death window) must succeed.
    let cfg = make_cfg(&[("STUB_DIE_AFTER", "1")]);
    let status = SttStatusHandle::new(SttStatus::Loading { backend: "cpu".into() });
    let sup = SttSupervisor::new(cfg, status.clone());
    sup.boot().await.expect("boot ok");

    let _ = sup
        .transcribe("req-1".into(), vec![0.0_f32; 256], "pt", "", None, 0)
        .await
        .expect("first transcribe ok");

    // The stub has now exited. Give the supervisor up to 2 s to respawn.
    let mut respawned = false;
    for _ in 0..40 {
        if matches!(status.get(), SttStatus::Ready { .. }) {
            respawned = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(respawned, "supervisor did not respawn: {:?}", status.get());

    // Clear the env var so the new child does NOT die again.
    std::env::remove_var("STUB_DIE_AFTER");

    let r2 = sup
        .transcribe("req-2".into(), vec![0.0_f32; 256], "pt", "", None, 0)
        .await
        .expect("second transcribe ok after respawn");
    assert_eq!(r2.request_id, "req-2");
}
```

> **Executor note**: the second test sets `STUB_DIE_AFTER` then removes it after the first request. This is racy if the supervisor respawns extremely fast and reads the env var before the test does the `remove_var`. The 500 ms `RESPAWN_DELAY_MS` gives us a comfortable window; if the test ever flakes, raise the delay or refactor to pass the `die-after` count via stdin.

- [ ] **Step 4: Build + run the supervisor test**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test stt_supervisor_test
```

Expected: 2 passed. The first run also builds `stub_worker.exe`; subsequent runs reuse it.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/tests/fixtures/stub_worker.rs src-tauri/tests/stt_supervisor_test.rs
git commit -m "test(stt): supervisor restart drill with stub worker fixture"
```

---

## Task 14: Wire utterance → STT in the capture controller

Phase 2's `CaptureController` writes WAV files. We add a **callback channel** to the controller that fires whenever a WAV is finalized; the main process listens, reads the WAV back to `Vec<f32>`, and calls `SttSupervisor::transcribe`.

We hand the supervisor + a Tauri `AppHandle` to the controller so it can emit `stt-transcription` events.

**Files:**
- Modify: `src-tauri/src/capture/controller.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Modify `CaptureController` to publish utterance events**

Replace the current `CaptureController` definition + worker_loop in `src-tauri/src/capture/controller.rs` with an extended version that emits an `UtteranceFinalized` event on a `crossbeam_channel::Sender<UtteranceFinalized>`. Keep the existing logic intact and only **add** the publish step.

Add this struct near the top (after `CaptureStatus`):

```rust
/// Emitted by the worker thread whenever an utterance WAV has been written.
#[derive(Debug, Clone)]
pub struct UtteranceFinalized {
    pub audio_path: PathBuf,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}
```

Change the `CaptureController` struct to carry the receiver-end of a `Sender<UtteranceFinalized>`:

```rust
pub struct CaptureController {
    cmd_tx: Sender<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    utterance_rx: Receiver<UtteranceFinalized>,
}
```

Change `spawn` to create the channel and pass the sender to `worker_loop`:

```rust
impl CaptureController {
    pub fn spawn(output_dir: PathBuf) -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let (utt_tx, utt_rx) = unbounded::<UtteranceFinalized>();
        let status = Arc::new(Mutex::new(CaptureStatus::Idle));
        let status_for_worker = status.clone();
        std::thread::Builder::new()
            .name("voicetabs-capture".into())
            .spawn(move || worker_loop(cmd_rx, status_for_worker, output_dir, utt_tx))
            .expect("spawn capture thread");
        Self { cmd_tx, status, utterance_rx: utt_rx }
    }

    pub fn start(&self) { let _ = self.cmd_tx.send(Cmd::Start); }
    pub fn stop(&self) { let _ = self.cmd_tx.send(Cmd::Stop); }
    pub fn status(&self) -> CaptureStatus { self.status.lock().clone() }

    /// Subscribe to utterance finalizations. The receiver is cheap to clone
    /// because crossbeam-channel receivers are MPMC.
    pub fn utterance_receiver(&self) -> Receiver<UtteranceFinalized> {
        self.utterance_rx.clone()
    }
}
```

Extend `worker_loop` to accept the sender:

```rust
fn worker_loop(
    cmd_rx: Receiver<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    output_dir: PathBuf,
    utt_tx: Sender<UtteranceFinalized>,
) {
    // ... existing setup unchanged ...
```

Replace the two `tracing::info!("wrote utterance WAV: ...")` blocks inside `process_chunks` with calls into a small helper that ALSO publishes on `utt_tx`. To pass the sender through, update `process_chunks`'s signature:

```rust
fn process_chunks(
    accumulator: &mut Vec<f32>,
    model: &mut VadModel,
    sm: &mut VadStateMachine,
    builder: &mut UtteranceBuilder,
    utt_tx: &Sender<UtteranceFinalized>,
) {
    while accumulator.len() >= CHUNK_SAMPLES {
        let chunk: Vec<f32> = accumulator.drain(..CHUNK_SAMPLES).collect();
        let prob = match model.predict(&chunk) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("VAD predict failed: {e}");
                continue;
            }
        };
        let ts_ms = unix_now_ms();
        if let Some(event) = sm.observe(prob, ts_ms) {
            let _ = builder.push_frame(&chunk);
            if let Some(path) = builder.on_vad_event(event) {
                publish_utterance(utt_tx, path, ts_ms);
            }
        } else if let Some(path) = builder.push_frame(&chunk) {
            publish_utterance(utt_tx, path, ts_ms);
            sm.force_idle();
        }
    }
}

fn publish_utterance(utt_tx: &Sender<UtteranceFinalized>, path: PathBuf, ended_at_ms: u64) {
    tracing::info!("wrote utterance WAV: {path:?}");
    // Pull start time from the filename if possible (`<unix_ms>.wav` format).
    let started_at_ms = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(ended_at_ms);
    let _ = utt_tx.send(UtteranceFinalized {
        audio_path: path,
        started_at_ms,
        ended_at_ms,
    });
}
```

And update the `select! { recv(frames_rx) -> frame => ... }` body to pass `&utt_tx` into `process_chunks`. Locate the existing line:

```rust
                    accumulator.extend_from_slice(&frame_16k);
                    process_chunks(&mut accumulator, model, &mut vad_sm, &mut builder);
```

Replace with:

```rust
                    accumulator.extend_from_slice(&frame_16k);
                    process_chunks(&mut accumulator, model, &mut vad_sm, &mut builder, &utt_tx);
```

Also export `UtteranceFinalized` from `src-tauri/src/capture/mod.rs`. Open `src-tauri/src/capture/mod.rs` and replace its contents:

```rust
pub mod controller;

pub use controller::{CaptureController, CaptureStatus, UtteranceFinalized};
```

- [ ] **Step 2: Compile**

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: clean. If you see "no field `utterance_rx` on ...", verify you replaced the struct AND the `spawn` constructor.

- [ ] **Step 3: Wire utterance → SttSupervisor → Tauri event in `lib.rs`**

We need to:
- Build a Tokio runtime to host the supervisor's async tasks.
- Spawn the supervisor at startup.
- Spawn a "drainer" Tokio task that consumes the controller's utterance channel, reads the WAV, calls `supervisor.transcribe()`, and emits a Tauri event.

Replace the body of `src-tauri/src/lib.rs` with:

```rust
pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;
pub mod stt;
pub mod utterance;
pub mod vad;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;

use crate::capture::UtteranceFinalized;
use crate::stt::gpu::Backend;
use crate::stt::status::SttStatus;
use crate::stt::{SttStatusHandle, SttSupervisor, SupervisorConfig};

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionEventPayload {
    pub request_id: String,
    pub text: String,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub duration_ms: u64,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub audio_path: String,
}

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
    let utt_rx = capture.utterance_receiver();

    // GPU autodetect (sync; uses settings cache).
    let backend = stt::gpu::detect_or_load(&db);

    let stt_status = SttStatusHandle::new(SttStatus::Loading {
        backend: backend.as_str().to_string(),
    });
    let stt_status_for_state = stt_status.clone();

    tauri::Builder::default()
        .manage(db)
        .manage(capture)
        .manage(stt_status_for_state)
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
            commands::stt::stt_status,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let model_path = resolve_model_path(&app_handle).expect("resolve model path");
            let worker_binary = resolve_worker_binary(&app_handle, backend)
                .expect("resolve worker binary");
            let cfg = SupervisorConfig {
                worker_binary,
                model_path,
                language: "pt".into(),
                backend,
            };
            let supervisor = SttSupervisor::new(cfg, stt_status.clone());
            app.manage(supervisor.clone());

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            // Leak the runtime to keep it alive for the app's lifetime. The
            // alternative — `manage(Arc<Runtime>)` — works too but adds an
            // extra `manage()` slot. A leaked runtime is fine for a desktop
            // app: it lives until the process exits.
            let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(rt));

            // Boot the supervisor.
            let sup_for_boot = supervisor.clone();
            rt.spawn(async move {
                if let Err(e) = sup_for_boot.boot().await {
                    tracing::error!("stt supervisor boot failed: {e}");
                }
            });

            // Drainer: consume utterances, transcribe, emit Tauri events.
            let sup_for_drainer = supervisor.clone();
            let app_for_drainer = app_handle.clone();
            rt.spawn(async move {
                drain_utterances(utt_rx, sup_for_drainer, app_for_drainer).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn resolve_model_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let p = app
        .path()
        .resolve(
            "resources/ggml-large-v3-turbo-q5_0.bin",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| anyhow::anyhow!("resolve model path: {e}"))?;
    if !p.exists() {
        return Err(anyhow::anyhow!(
            "model file not found at {}; did you run the Task 2 download?",
            p.display()
        ));
    }
    Ok(p)
}

fn resolve_worker_binary(
    app: &tauri::AppHandle,
    backend: Backend,
) -> anyhow::Result<PathBuf> {
    let name = match backend {
        Backend::Cuda => "stt_worker_cuda",
        Backend::Cpu => "stt_worker_cpu",
    };
    // In dev mode, the worker lives next to voicetabs.exe under target/.
    // In a bundled installer it lives in the resource dir as an externalBin.
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        // Bundled: alongside the main exe.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                v.push(dir.join(format!("{name}.exe")));
            }
        }
        // Dev: cargo workspace target.
        v.push(PathBuf::from(format!("../target/debug/{name}.exe")));
        v.push(PathBuf::from(format!("../target/release/{name}.exe")));
        v.push(PathBuf::from(format!("target/debug/{name}.exe")));
        v.push(PathBuf::from(format!("target/release/{name}.exe")));
        v
    };
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(anyhow::anyhow!(
        "could not locate {name}.exe; checked: {:?}",
        candidates
    ))
}

async fn drain_utterances(
    utt_rx: crossbeam_channel::Receiver<UtteranceFinalized>,
    sup: SttSupervisor,
    app: tauri::AppHandle,
) {
    // crossbeam_channel is sync; we move blocking recv onto a dedicated thread
    // and forward into an async channel.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<UtteranceFinalized>();
    std::thread::Builder::new()
        .name("voicetabs-utt-bridge".into())
        .spawn(move || {
            while let Ok(u) = utt_rx.recv() {
                if tx.send(u).is_err() {
                    break;
                }
            }
        })
        .expect("spawn utt bridge");

    while let Some(u) = rx.recv().await {
        let sup = sup.clone();
        let app = app.clone();
        // Spawn per-utterance so a slow transcription doesn't block the queue.
        tokio::spawn(async move {
            let samples = match read_wav_samples(&u.audio_path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("read wav {}: {e}", u.audio_path.display());
                    return;
                }
            };
            let request_id = uuid::Uuid::new_v4().to_string();
            match sup
                .transcribe(
                    request_id.clone(),
                    samples,
                    "pt",
                    "", // initial_prompt — populated from settings.vocab_terms in Phase 4
                    Some(u.audio_path.clone()),
                    u.started_at_ms,
                )
                .await
            {
                Ok(r) => {
                    tracing::info!(
                        "transcription request={} text={:?} ({}ms)",
                        r.request_id,
                        r.text,
                        r.duration_ms
                    );
                    let payload = TranscriptionEventPayload {
                        request_id: r.request_id,
                        text: r.text,
                        avg_logprob: r.avg_logprob,
                        no_speech_prob: r.no_speech_prob,
                        duration_ms: r.duration_ms,
                        started_at_ms: u.started_at_ms,
                        ended_at_ms: u.ended_at_ms,
                        audio_path: u.audio_path.to_string_lossy().to_string(),
                    };
                    if let Err(e) = app.emit("stt-transcription", &payload) {
                        tracing::warn!("emit stt-transcription failed: {e}");
                    }
                }
                Err(e) => {
                    tracing::error!("transcription failed for {}: {e}", request_id);
                }
            }
        });
    }
    let _ = Arc::<()>::new(()); // suppress unused import lint for Arc if needed
}

fn read_wav_samples(path: &std::path::Path) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 {
        return Err(anyhow::anyhow!(
            "unexpected wav spec: rate={} channels={}",
            spec.sample_rate,
            spec.channels
        ));
    }
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / 32_767.0))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(samples)
}
```

- [ ] **Step 4: `cargo check`**

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: clean. Common pitfalls:
- `app.path().resolve(...)` returns a Tauri 2 `PathBuf` future — it's sync here. If the executor sees a different API (`path_resolver()` from Tauri 1), the project is on Tauri 2 per the foundation plan; cross-check `src-tauri/Cargo.toml`.
- `Manager::emit` is the Tauri 2 method on `AppHandle`. If the trait isn't in scope, ensure `use tauri::Manager;` is at the top of `lib.rs`.

- [ ] **Step 5: Re-run the full test suite to confirm no regressions**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all prior + new tests pass. Running totals after each task: 37 Phase 2 baseline → +4 stt::framing (Task 9) = 41 → +4 stt::gpu (Task 10) = 45 → +2 stt::supervisor (Task 13) = 47. After this Task 14, no new Rust tests are added, so the count stays at **47**.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/capture src-tauri/src/lib.rs
git commit -m "feat(stt): wire utterance->transcribe; emit stt-transcription events"
```

---

## Task 15: Integration round-trip test (`stt_worker/tests/round_trip.rs`)

The supervisor test uses a stub. This task adds the **real** test: spawn `stt_worker_cpu.exe`, feed it a small WAV, assert the response framing parses.

We do NOT assert on transcription text content (whisper output is sensitive to model + audio) — only that:
- The handshake frame arrives within 60 s.
- A response frame arrives for our request_id.
- Either `text` is non-empty (with a real speech sample) OR the response carries an `error` field that we surface (with a sine sample).

**Files:**
- Create: `src-tauri/tests/fixtures/speech_pt.wav` (recorded by engineer; see Step 1)
- Create: `stt_worker/tests/round_trip.rs`

- [ ] **Step 1: Provide a speech WAV at `src-tauri/tests/fixtures/speech_pt.wav`**

The fixture is a 3–5 second 16 kHz mono 16-bit PCM WAV containing one Brazilian Portuguese sentence (e.g. "Esta é uma frase de teste para o sistema de transcrição"). Three options for the engineer:

   1. **Record one with `cpal`-based dev mode** (preferred):
      - Run `npm run tauri dev`.
      - Click capture, speak a sentence, click capture off.
      - Find the WAV at `%APPDATA%\voicetabs\audio\<unix_ms>.wav`.
      - Copy it to `src-tauri/tests/fixtures/speech_pt.wav`.

   2. **Synthesize with Windows TTS** (PowerShell, requires SAPI):
      ```powershell
      Add-Type -AssemblyName System.Speech
      $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
      $synth.SelectVoice("Microsoft Maria Desktop")  # Brazilian Portuguese; may differ on your system. List with: $synth.GetInstalledVoices() | %{ $_.VoiceInfo.Name }
      $synth.SetOutputToWaveFile("$PWD\src-tauri\tests\fixtures\speech_pt.wav")
      $synth.Speak("Esta é uma frase de teste para o sistema de transcrição.")
      $synth.Dispose()
      # Resample to 16 kHz mono if needed:
      # Use ffmpeg if available:
      #   ffmpeg -i src-tauri\tests\fixtures\speech_pt.wav -ar 16000 -ac 1 -sample_fmt s16 src-tauri\tests\fixtures\speech_pt_16k.wav -y
      #   Move-Item -Force src-tauri\tests\fixtures\speech_pt_16k.wav src-tauri\tests\fixtures\speech_pt.wav
      ```

   3. **Fallback (no speech available): use a sine wave**. The test will detect this and assert weaker properties:
      ```powershell
      # Generate a 3 s 440 Hz sine at 16 kHz mono, 16-bit PCM.
      $rate = 16000; $secs = 3; $freq = 440.0
      $n = $rate * $secs
      $bytes = New-Object byte[] (44 + 2 * $n)
      # Minimal WAV header
      [byte[]] $hdr = @(
        0x52,0x49,0x46,0x46, 0,0,0,0, 0x57,0x41,0x56,0x45,
        0x66,0x6d,0x74,0x20, 16,0,0,0, 1,0, 1,0,
        0x80,0x3e,0,0,   0,0x7d,0,0,   2,0, 16,0,
        0x64,0x61,0x74,0x61, 0,0,0,0)
      [Array]::Copy($hdr, $bytes, 44)
      $size = 36 + 2 * $n
      [BitConverter]::GetBytes([UInt32]$size).CopyTo($bytes, 4)
      [BitConverter]::GetBytes([UInt32](2 * $n)).CopyTo($bytes, 40)
      for ($i = 0; $i -lt $n; $i++) {
        $v = [int]([Math]::Sin(2 * [Math]::PI * $freq * $i / $rate) * 0.3 * 32767)
        [BitConverter]::GetBytes([Int16]$v).CopyTo($bytes, 44 + 2 * $i)
      }
      [System.IO.File]::WriteAllBytes("$PWD\src-tauri\tests\fixtures\speech_pt.wav", $bytes)
      ```
      The test detects "this is the sine fallback" by checking if `text` comes back empty AND `no_speech_prob` is high; in that case it asserts the framing was valid and stops. Document in the test that this is a known limitation.

   The fixture file IS committed (~96 KB at 3 s × 16 kHz × 2 bytes/sample). Do not commit longer-than-5-second clips.

- [ ] **Step 2: Write `stt_worker/tests/round_trip.rs`**

```rust
//! Real subprocess integration test. Requires the bundled Whisper model at
//! `src-tauri/resources/ggml-large-v3-turbo-q5_0.bin` and the fixture WAV at
//! `src-tauri/tests/fixtures/speech_pt.wav`.
//!
//! Tagged `#[ignore]` by default because it takes ~30 s to load the model and
//! ~1–5 s to transcribe. Run explicitly with:
//!     cargo test -p stt_worker --test round_trip -- --ignored

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR for the test = stt_worker/. Workspace root is parent.
    p.pop();
    p
}

fn model_path() -> PathBuf {
    workspace_root().join("src-tauri/resources/ggml-large-v3-turbo-q5_0.bin")
}

fn fixture_wav() -> PathBuf {
    workspace_root().join("src-tauri/tests/fixtures/speech_pt.wav")
}

fn worker_bin() -> PathBuf {
    workspace_root().join("target/debug/stt_worker_cpu.exe")
}

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

fn read_wav(path: &std::path::Path) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).expect("open fixture wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "fixture must be 16 kHz");
    assert_eq!(spec.channels, 1, "fixture must be mono");
    reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32_767.0)
        .collect()
}

#[test]
#[ignore]
fn round_trip_real_worker() {
    if !model_path().exists() {
        eprintln!(
            "skipping: model file not present at {}",
            model_path().display()
        );
        return;
    }
    if !fixture_wav().exists() {
        eprintln!(
            "skipping: fixture WAV not present at {}",
            fixture_wav().display()
        );
        return;
    }
    if !worker_bin().exists() {
        panic!(
            "stt_worker_cpu.exe not built; run `cargo build -p stt_worker` first ({})",
            worker_bin().display()
        );
    }

    let mut child = Command::new(worker_bin())
        .args(["--model", &model_path().to_string_lossy(), "--language", "pt"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn worker");

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    // Drain stderr in a thread so the worker doesn't block on a full pipe.
    std::thread::spawn(move || {
        let r = BufReader::new(stderr);
        for line in r.lines().flatten() {
            eprintln!("[worker] {line}");
        }
    });

    let mut stdin = std::io::BufWriter::new(stdin);
    let mut stdout = std::io::BufReader::new(stdout);

    // 1. Read ready frame.
    let ready_bytes = read_frame(&mut stdout).expect("ready frame");
    let ready: serde_json::Value = serde_json::from_slice(&ready_bytes).unwrap();
    assert_eq!(ready["ready"], true);

    // 2. Send request header + PCM.
    let samples = read_wav(&fixture_wav());
    let header = format!(
        r#"{{"request_id":"req-1","sample_rate":16000,"language":"pt","initial_prompt":"","n_samples":{}}}"#,
        samples.len()
    );
    write_frame(&mut stdin, header.as_bytes());
    let mut pcm = Vec::with_capacity(samples.len() * 4);
    for s in &samples {
        pcm.extend_from_slice(&s.to_le_bytes());
    }
    write_frame(&mut stdin, &pcm);
    // Close stdin to signal "no more requests" — this is fine because we want
    // the worker to exit cleanly after responding.
    drop(stdin);

    // 3. Read response.
    let resp_bytes = read_frame(&mut stdout).expect("response frame");
    let resp: serde_json::Value = serde_json::from_slice(&resp_bytes).unwrap();
    assert_eq!(resp["request_id"], "req-1");

    // 4. Validate: either a non-empty text (speech sample) or an explicit
    //    error/empty-text scenario (sine wave fallback).
    if let Some(text) = resp["text"].as_str() {
        if !text.trim().is_empty() {
            eprintln!("transcribed text: {text:?}");
        } else {
            eprintln!("empty text — fixture is likely the sine fallback");
        }
    } else if let Some(err) = resp["error"].as_str() {
        panic!("worker returned error: {err}");
    } else {
        panic!("response had neither text nor error: {resp}");
    }

    // 5. Worker exits cleanly on stdin EOF.
    let status = child.wait().expect("wait worker");
    assert!(status.success(), "worker exited with {status:?}");
}
```

- [ ] **Step 3: Run the round-trip test (manually, opt-in)**

```powershell
cargo build -p stt_worker
cargo test -p stt_worker --test round_trip -- --ignored
```

Expected: one passed (or "skipping" if the model is missing — only on a fresh checkout that hasn't run Task 2). With the real speech sample, the transcribed text should be a recognizable Portuguese phrase printed to stderr.

- [ ] **Step 4: Commit (the fixture WAV is small; commit it)**

```powershell
git add stt_worker/tests/round_trip.rs src-tauri/tests/fixtures/speech_pt.wav
git commit -m "test(stt_worker): real subprocess round-trip with sample WAV"
```

> **Note**: if the fixture is the sine fallback, the test still passes but produces empty text. Document this in a `// FIXME` comment in the test so a future engineer knows to drop in a real speech clip.

---

## Task 16: Bundle the worker binaries + model in `tauri.conf.json`

Tauri's bundler needs to know about both. `bundle.externalBin` ships executables next to the main app; `bundle.resources` ships read-only data files into the resource dir.

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Update `src-tauri/tauri.conf.json`**

Replace its contents with:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "VoiceTabs",
  "version": "0.1.0",
  "identifier": "com.voicetabs.app",
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "VoiceTabs",
        "width": 1100,
        "height": 720,
        "minWidth": 800,
        "minHeight": 500,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/icon.ico", "icons/icon.png"],
    "externalBin": [
      "../target/release/stt_worker_cpu",
      "../target/release/stt_worker_cuda"
    ],
    "resources": [
      "resources/ggml-large-v3-turbo-q5_0.bin"
    ],
    "windows": {
      "nsis": {
        "displayLanguageSelector": false,
        "languages": ["English", "PortugueseBR"]
      }
    }
  }
}
```

Notes:
- `externalBin` paths are resolved relative to `src-tauri/`. Tauri's bundler appends the host triple (e.g. `-x86_64-pc-windows-msvc.exe`) when looking for the actual file; it then renames at install time so the file ends up next to `voicetabs.exe` as `stt_worker_cpu.exe`.
- The dev runtime resolution (Task 14's `resolve_worker_binary`) checks multiple candidates including the workspace `target/debug/` and `target/release/` dirs, so `npm run tauri dev` works as long as a `cargo build -p stt_worker` (and, for CUDA testing, `cargo build -p stt_worker --features cuda`) has been run.
- The model file (`resources/...`) lands inside the installed app's resource dir and is resolved by `app.path().resolve(...)` in Task 14.

- [ ] **Step 2: For CUDA — ensure `cudart64_12.dll` + `cublas64_12.dll` are discoverable**

`whisper-rs 0.13` with the `cuda` feature links against the CUDA runtime. The DLLs ship with the CUDA Toolkit; on a developer machine they are on `PATH` automatically. **On the end-user's machine, they are NOT.** Two acceptable shipping strategies:

   1. **Static link** (preferred if `whisper-rs` exposes it): set `CUDA_RUNTIME_LIB=static` in `stt_worker/build.rs` or via the `static-cuda` feature if available. Check the `whisper-rs` 0.13 docs at execution time; if a static-linking feature exists, enable it in `stt_worker/Cargo.toml`'s `cuda` feature definition.

   2. **Ship the DLLs as resources**: copy `cudart64_12.dll` + `cublas64_12.dll` + `cublasLt64_12.dll` (~150 MB combined) from a CUDA Toolkit install (typically `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.x\bin\`) to `src-tauri/resources/cuda/`, add them to the `bundle.resources` list, and at runtime call `SetDllDirectoryW` (or copy them to the same dir as `stt_worker_cuda.exe`) before spawning the worker.

   **For Phase 3, document the choice in this file and proceed with whichever is faster.** If neither is necessary because the developer's machine has the CUDA Toolkit installed AND the manual acceptance is run on the same machine, just note that the installer-on-a-clean-VM acceptance (a Phase 8 task) will surface the gap.

   Add to `src-tauri/resources/.gitkeep` a comment block (or a `README.md` next to it) explaining the DLL situation if Option 2 is chosen.

- [ ] **Step 3: Sanity-check the dev path works**

```powershell
cargo build -p stt_worker
npm run tauri dev
```

Expected: the app opens. The footer status dot (Task 17, not yet implemented) is absent for now; check the log for "loading model" lines and a subsequent "ready" status update. **Note**: model load takes ~10–30 s on the reference hardware on the first launch (cold disk cache); subsequent launches are ~1–2 s.

If the dev build fails because `tauri-build` doesn't find `target/release/stt_worker_cpu` (we only built debug), set the `TAURI_SKIP_SIDECAR_VALIDATION` env var when running dev:

```powershell
$env:TAURI_SKIP_SIDECAR_VALIDATION = "1"
npm run tauri dev
```

…or, more reliably, only enable `externalBin` for release builds by gating it via a `tauri.conf.production.json` override (Tauri 2 supports config merging). For Phase 3 we accept the workaround and add the gating as a TODO for Phase 8.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/tauri.conf.json
git commit -m "build(tauri): bundle stt_worker binaries + whisper model"
```

---

## Task 17: `stt_status` command + status dot frontend

A small command + a small frontend dot.

**Files:**
- Create: `src-tauri/src/commands/stt.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src/lib/tauri.ts`
- Create: `src/stores/sttStore.ts`
- Create: `src/components/SttStatusDot.tsx`
- Create: `src/__tests__/SttStatusDot.test.tsx`
- Modify: `src/i18n/locales/pt-BR.json`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/App.tsx`
- Modify: `src/__tests__/i18n.test.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: Create `src-tauri/src/commands/stt.rs`**

```rust
use tauri::State;

use crate::stt::{SttStatus, SttStatusHandle};

use super::tabs::CommandError;

#[tauri::command]
pub fn stt_status(status: State<'_, SttStatusHandle>) -> Result<SttStatus, CommandError> {
    Ok(status.get())
}
```

- [ ] **Step 2: Update `src-tauri/src/commands/mod.rs`**

```rust
pub mod capture;
pub mod settings;
pub mod stt;
pub mod tabs;
```

(The handler was already registered in Task 14's `lib.rs` snippet under `tauri::generate_handler![..., commands::stt::stt_status]`. Verify.)

- [ ] **Step 3: Update `src/lib/tauri.ts`** — append after `captureApi`:

```ts
export type SttStatus =
  | { state: "loading"; backend: string }
  | { state: "ready"; backend: string; model_id: string }
  | { state: "restarting"; backend: string }
  | { state: "error"; message: string };

export const sttApi = {
  status(): Promise<SttStatus> {
    return invoke<SttStatus>("stt_status");
  },
};

export type TranscriptionEvent = {
  request_id: string;
  text: string;
  avg_logprob: number;
  no_speech_prob: number;
  duration_ms: number;
  started_at_ms: number;
  ended_at_ms: number;
  audio_path: string;
};
```

- [ ] **Step 4: Create `src/stores/sttStore.ts`**

```ts
import { create } from "zustand";

import { sttApi, SttStatus } from "../lib/tauri";

type SttState = {
  status: SttStatus;
  pollHandle: ReturnType<typeof setInterval> | null;

  refresh: () => Promise<void>;
  startPolling: () => void;
  stopPolling: () => void;
};

const POLL_INTERVAL_MS = 1500;

export const useSttStore = create<SttState>((set, get) => ({
  status: { state: "loading", backend: "unknown" },
  pollHandle: null,

  async refresh() {
    try {
      const status = await sttApi.status();
      set({ status });
    } catch (e) {
      set({ status: { state: "error", message: String(e) } });
    }
  },

  startPolling() {
    if (get().pollHandle !== null) return;
    const handle = setInterval(() => {
      void get().refresh();
    }, POLL_INTERVAL_MS);
    set({ pollHandle: handle });
  },

  stopPolling() {
    const h = get().pollHandle;
    if (h !== null) {
      clearInterval(h);
      set({ pollHandle: null });
    }
  },
}));
```

- [ ] **Step 5: Create the failing test in `src/__tests__/SttStatusDot.test.tsx`**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SttStatusDot } from "../components/SttStatusDot";
import { SttStatus } from "../lib/tauri";

describe("SttStatusDot", () => {
  it("renders a green dot when ready", () => {
    const status: SttStatus = { state: "ready", backend: "cpu", model_id: "x" };
    render(<SttStatusDot status={status} />);
    const dot = screen.getByTestId("stt-dot");
    expect(dot).toHaveClass("stt-dot--ready");
  });

  it("renders a yellow dot when loading", () => {
    const status: SttStatus = { state: "loading", backend: "cpu" };
    render(<SttStatusDot status={status} />);
    expect(screen.getByTestId("stt-dot")).toHaveClass("stt-dot--loading");
  });

  it("renders a yellow dot when restarting", () => {
    const status: SttStatus = { state: "restarting", backend: "cpu" };
    render(<SttStatusDot status={status} />);
    expect(screen.getByTestId("stt-dot")).toHaveClass("stt-dot--restarting");
  });

  it("renders a red dot when error", () => {
    const status: SttStatus = { state: "error", message: "boom" };
    render(<SttStatusDot status={status} />);
    expect(screen.getByTestId("stt-dot")).toHaveClass("stt-dot--error");
  });

  it("exposes the status text via title attribute for tooltip", () => {
    const status: SttStatus = { state: "ready", backend: "cuda", model_id: "ggml-large-v3-turbo-q5_0" };
    render(<SttStatusDot status={status} />);
    const dot = screen.getByTestId("stt-dot");
    expect(dot).toHaveAttribute("title");
    expect(dot.getAttribute("title")).toMatch(/cuda/);
  });
});
```

- [ ] **Step 6: Run the tests to confirm failure**

```powershell
npm test -- src/__tests__/SttStatusDot.test.tsx
```

Expected: 5 failing (no module).

- [ ] **Step 7: Create `src/components/SttStatusDot.tsx`**

```tsx
import { useTranslation } from "react-i18next";

import { SttStatus } from "../lib/tauri";

type Props = { status: SttStatus };

export function SttStatusDot({ status }: Props) {
  const { t } = useTranslation();
  let cls = "stt-dot";
  let title = "";

  switch (status.state) {
    case "ready":
      cls += " stt-dot--ready";
      title = `${t("stt.ready")} (${status.backend} · ${status.model_id})`;
      break;
    case "loading":
      cls += " stt-dot--loading";
      title = `${t("stt.loading")} (${status.backend})`;
      break;
    case "restarting":
      cls += " stt-dot--restarting";
      title = `${t("stt.restarting")} (${status.backend})`;
      break;
    case "error":
      cls += " stt-dot--error";
      title = `${t("stt.error")}: ${status.message}`;
      break;
  }

  return <span data-testid="stt-dot" className={cls} title={title} />;
}
```

- [ ] **Step 8: Append styles to `src/styles.css`**

```css

.stt-dot {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  margin: 0 8px;
  vertical-align: middle;
  background: #888;
}
.stt-dot--ready { background: #5cd97c; }
.stt-dot--loading { background: #ffce5c; }
.stt-dot--restarting { background: #ffce5c; }
.stt-dot--error { background: #ff6b6b; }
```

- [ ] **Step 9: Append i18n strings**

To `src/i18n/locales/pt-BR.json`, add the `stt` block before the closing `}`:

```json
,
  "stt": {
    "ready": "Pronto",
    "loading": "Carregando modelo",
    "restarting": "Reiniciando",
    "error": "Erro"
  }
```

To `src/i18n/locales/en.json`:

```json
,
  "stt": {
    "ready": "Ready",
    "loading": "Loading model",
    "restarting": "Restarting",
    "error": "Error"
  }
```

> **Executor note**: the JSON files use trailing-comma-free syntax. When you add the new block, ensure the previous block (`capture` in pt-BR.json) has a trailing comma after its closing `}` before the `"stt":` key.

- [ ] **Step 10: Update `src/App.tsx`** — mount the dot in the footer and start polling.

Find the existing imports section and add:

```tsx
import { SttStatusDot } from "./components/SttStatusDot";
import { useSttStore } from "./stores/sttStore";
```

Add to the destructured hooks at the top of `App()`:

```tsx
const stt = useSttStore();
```

Augment the bootstrap `useEffect` to start STT polling:

```tsx
useEffect(() => {
  void (async () => {
    await settings.load();
    await tabs.load();
    await capture.refresh();
    capture.startPolling();
    await stt.refresh();
    stt.startPolling();
  })();
  return () => {
    capture.stopPolling();
    stt.stopPolling();
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
}, []);
```

In the footer JSX, insert the dot between the CaptureToggle and the settings gear:

```tsx
<footer className="app-footer">
  <CaptureToggle
    status={capture.status}
    onStart={() => void capture.startCapture()}
    onStop={() => void capture.stopCapture()}
  />
  <SttStatusDot status={stt.status} />
  <button onClick={settings.openDrawer} className="footer-button">
    ⚙ {t("settings.open")}
  </button>
</footer>
```

- [ ] **Step 11: Update the i18n test mock** in `src/__tests__/i18n.test.tsx`

Inside the existing `vi.mock("@tauri-apps/api/core", ...)` block, add a new branch for `stt_status` before the final fallback:

```ts
if (command === "stt_status") {
  return { state: "ready", backend: "cpu", model_id: "stub" };
}
```

- [ ] **Step 12: Run the TS suite**

```powershell
npm test
```

Expected: 13 existing + 5 new = **18** passed.

- [ ] **Step 13: Compile + run Rust tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all green (47 in main crate; unchanged by this task — it adds Rust code but no new tests).

- [ ] **Step 14: Commit**

```powershell
git add src-tauri/src/commands src/lib/tauri.ts src/stores/sttStore.ts src/components/SttStatusDot.tsx src/__tests__ src/App.tsx src/styles.css src/i18n/locales
git commit -m "feat(ui): stt_status command + footer status dot indicator"
```

---

## Task 18: Manual acceptance pass

The Phase 3 manual gate. The executor can do a few automatic checks; the human sign-off validates the end-to-end transcription.

**Files:** None new. Optional `docs/phase-3-acceptance.md` like Phase 2's.

- [ ] **Step 1: Reset state so the test starts clean**

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs\audio" -ErrorAction SilentlyContinue
Remove-Item -Force "$env:APPDATA\voicetabs\logs\voicetabs*.log" -ErrorAction SilentlyContinue
# Clear the cached backend choice so we exercise the probe path.
$db = "$env:APPDATA\voicetabs\voicetabs.db"
if (Test-Path $db) {
    # Best-effort: drop the cached backend setting if SQLite cli is available;
    # otherwise we accept that the cache is used (which is fine for acceptance).
    if (Get-Command sqlite3 -ErrorAction SilentlyContinue) {
        sqlite3 $db "DELETE FROM settings WHERE key='stt_backend';"
    }
}
```

- [ ] **Step 2: Confirm both builds compile**

```powershell
cargo build -p stt_worker
# CUDA build is OPTIONAL during this step — only run if a CUDA toolchain is installed.
# cargo build -p stt_worker --features cuda
cargo build --manifest-path src-tauri\Cargo.toml --release
```

Expected: success.

- [ ] **Step 3: Confirm the full test suite is green**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
npm test
```

Expected: Rust **47** in `voicetabs_lib` + **11** in `stt_worker` = 58 total. TS **18** passed.

- [ ] **Step 4: Manual transcription drill**

```powershell
npm run tauri dev
```

- The app opens.
- Footer shows a **yellow dot** (loading) for ~10–30 s, then **green** (ready).
- Click the capture toggle. The mic indicator appears.
- Speak one short PT-BR sentence (e.g. "Olá VoiceTabs, este é um teste"). Pause.
- The log at `%APPDATA%\voicetabs\logs\voicetabs.YYYY-MM-DD` (rolling daily file) should contain a line like:
  ```
  INFO ...  voicetabs_lib: transcription request=... text="Olá VoiceTabs, este é um teste." (250ms)
  ```
- Open the WebView devtools (right-click in the window → Inspect → Console) and look for a `stt-transcription` event log if you've added a temporary `listen` in the frontend; otherwise the log file is sufficient.

- [ ] **Step 5: Kill-and-recover drill (A7 partial)**

In a separate terminal while the app is running:

```powershell
Get-Process | Where-Object { $_.ProcessName -match "stt_worker" } | Stop-Process -Force
```

- The footer dot turns **yellow** (restarting) within ~1 s.
- Within ~5–10 s it goes **green** again.
- Speak another sentence. It is transcribed normally.

(Full A7 — 5 s budget end-to-end — is rigorously validated in Phase 7. Here we only confirm the supervisor recovers at all.)

- [ ] **Step 6: Stop-and-report**

Write a short summary covering:
- Which backend was chosen by the probe (`cuda` / `cpu`).
- The cold-start model-load latency.
- One example transcribed sentence (the log line).
- Whether the kill-and-recover drill succeeded.
- Any failures or known issues to address in Phase 4 polish.

- [ ] **Step 7: Push to remote**

```powershell
git push origin master
```

CI must stay green. If it fails because the runner doesn't have the model file (which is correct — the model isn't committed), the runner skips the `--ignored` round-trip test and runs everything else. Confirm by watching `gh run watch`.

---

## End of Phase 3 — checkpoint

**Stop here and produce a stop-and-report.** Then we plan Phase 4 (utterance → segments table → tab routing → hallucination filter) in a separate session.

**What's verified at this checkpoint:**
- A1, A2 — **A2 is intentionally NOT applicable** (no wizard; the model is bundled). A1 (double-click installer works) becomes testable once Phase 8 produces a signed-or-unsigned installer.
- A3, A4, A5, A6 — not yet (Phase 4+ wires text to tabs; Phase 5 owns hotkeys).
- A7 (kill stt_worker, recover within 5 s) — **partial**: supervisor restart loop is in place and the manual drill passes on warm cache. Cold-cache budgets are not yet measured; Phase 7 owns that.
- A8 — tabs + settings still persist; audio WAVs accumulate as before. `stt_backend` is now also persisted.
- L1, L2 — **partial**: end-to-end latency is observed on the reference hardware but not yet enforced or measured rigorously; Phase 4 will surface it once segments render in the UI.

**What's verified that isn't in the acceptance list but matters:**
- IPC framing has unit tests on both sides of the wire (7 in worker + 4 in main + 4 protocol tests).
- Supervisor restart logic has an integration test against a deterministic stub worker.
- `whisper-rs` real-worker round-trip has an `--ignored` integration test for opt-in pre-commit verification.
- GPU autodetection is layered (env override → cache → nvidia-smi → wmic → CPU) and cached in `settings`.
- The frontend shows a live status dot driven by polling; a `stt-transcription` event already fires per utterance for Phase 4 to consume.

**Total Rust test count after this phase:**
- Main crate (`voicetabs_lib`): 37 Phase 2 baseline + 4 `stt::framing` (sync + async smoke) + 4 `stt::gpu` + 2 `stt::supervisor` = **47**.
- Worker crate (`stt_worker`): 7 `framing` + 4 `protocol` = **11**. Plus the `--ignored` `round_trip` test (1) which only runs on explicit opt-in.
- Combined: **58** in default `cargo test`, **59** with `--ignored`.

**Total TS test count:** 18 (13 Phase 2 baseline + 5 SttStatusDot).
