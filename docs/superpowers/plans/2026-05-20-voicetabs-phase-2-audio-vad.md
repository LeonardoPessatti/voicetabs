# VoiceTabs — Phase 2 (Audio Capture + VAD) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture microphone audio at 16 kHz mono, run Silero VAD to detect speech, and write each detected utterance as a `<unix_ms>.wav` file under `%APPDATA%\voicetabs\audio\`. Capture is toggled from a footer button. **No STT, no tab routing** — those are Phase 3+ and Phase 4 respectively.

**Architecture:** A `CaptureController` owns a single long-running worker thread. The thread holds the cpal `Stream` (which is `!Send` on Windows), receives `Start`/`Stop` commands from the Tauri main process via a `crossbeam` channel, receives audio frames from the cpal callback through a second `crossbeam` channel, and runs the VAD + utterance pipeline inline. VAD uses the `voice_activity_detector` crate (Silero v5 bundled, `ort` under the hood with statically linked ONNX Runtime via `download-binaries`). A pure-Rust state machine layered on top of the model adds hysteresis. The utterance builder maintains a 500 ms pre-roll ring, accumulates frames between VAD edges, and writes a 16 kHz mono 16-bit PCM WAV on each falling edge (or 30 s cap).

**Tech Stack additions:** `cpal 0.15` · `voice_activity_detector 0.2` · `ort 2` (download-binaries) · `hound 3.5` · `crossbeam-channel 0.5`.

**Reference:** Spec `docs/superpowers/specs/2026-05-20-voicetabs-design.md` §5.2 (dataflow), §7.1 (AudioInput), §7.2 (VAD), §7.3 (UtteranceBuilder).

**Builds on:** Phase 0+1 plan `docs/superpowers/plans/2026-05-20-voicetabs-foundation.md`. Starting state: HEAD `601a9b1` on `master` branch.

---

## Acceptance for this plan

- The app opens with capture **OFF**; a footer toggle says `🎤 Capturar: OFF` (PT-BR) / `🎤 Capture: OFF` (EN).
- Clicking the toggle flips it to `ON`; the operating system shows the mic-in-use indicator.
- Speaking a sentence into the mic and then pausing for ~1 s produces exactly one WAV file at `%APPDATA%\voicetabs\audio\<unix_ms>.wav`, sample-rate 16 kHz, mono, 16-bit, containing the utterance plus ~500 ms of leading pre-roll.
- Speaking a short follow-up after another pause produces a second WAV file. The first file is untouched.
- 60 seconds of silence with capture ON produces **zero** WAV files. (Spec A5 partial.)
- `cargo test` passes — at minimum the VAD state machine tests and the WAV writer tests (which don't require an audio device).
- `npm test` passes (8 existing tests stay green).
- `cargo build --release` still produces `target/release/voicetabs.exe`.
- CI on master stays green after push.

## Out of scope (deferred to later plans)

- STT (Phase 3); first-run model wizard (Phase 3); subprocess supervisor (Phase 7).
- Tab routing of utterances to a specific tab (Phase 4); hallucination text filter (Phase 4); segments table writes (Phase 4); segment UI cards (Phase 6).
- Hotkey-driven start/stop (Phase 5); always-on vs PTT mode selector (Phase 5); system tray (Phase 5).
- Microphone device picker in Settings (Phase 5 or later); we use the system default input.
- Device hot-swap recovery (`audio-status: lost`), mic-permission-denied instructional UI — both flagged in spec §7.1 but deferred; if `cpal::default_host().default_input_device()` returns `None`, we surface a single error log line and the capture stays OFF.
- Hardware sample rates other than 16 kHz. We request 16 kHz directly from cpal. WASAPI's shared-mode mixer resamples automatically on Windows, so this is fine on the reference hardware. If a device refuses, we log and abort that start attempt.
- Vocabulary biasing, custom prompts — not relevant without STT.

---

## File structure after this plan

```
src-tauri/
├── Cargo.toml                            # +5 deps (cpal, voice_activity_detector, ort, hound, crossbeam-channel)
└── src/
    ├── audio/
    │   ├── mod.rs                        # re-exports
    │   ├── devices.rs                    # enumerate input devices
    │   └── input.rs                      # build cpal Stream, push frames into a channel
    ├── vad/
    │   ├── mod.rs
    │   ├── model.rs                      # wrap voice_activity_detector
    │   └── state.rs                      # hysteresis state machine (TDD)
    ├── utterance/
    │   ├── mod.rs
    │   ├── preroll.rs                    # ring buffer of recent f32 samples
    │   ├── wav.rs                        # hound writer wrapper (TDD)
    │   └── builder.rs                    # combine preroll + VAD edges → WAV file (TDD)
    ├── capture/
    │   ├── mod.rs
    │   └── controller.rs                 # CaptureController + worker thread
    ├── commands/
    │   └── capture.rs                    # Tauri commands: capture_start/stop/status
    ├── paths.rs                          # MODIFIED: + audio_dir()
    └── lib.rs                            # MODIFIED: register controller, register commands

src/                                      # frontend
├── lib/tauri.ts                          # MODIFIED: + captureApi
├── i18n/locales/{pt-BR,en}.json          # MODIFIED: + capture.* keys
├── stores/captureStore.ts                # NEW: capture state + polling
├── components/
│   └── CaptureToggle.tsx                 # NEW: footer toggle button
└── App.tsx                               # MODIFIED: mount toggle in footer
```

Each module has a single responsibility: `audio/` deals with cpal only, `vad/` with Silero only, `utterance/` with assembling and persisting WAVs, `capture/` orchestrates the three. `commands/capture.rs` is the IPC boundary.

---

# Phase 2 tasks

## Task 1: Add Phase 2 Rust dependencies

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Read the current `[dependencies]` section so you can insert the new ones cleanly**

```powershell
Get-Content src-tauri\Cargo.toml | Select-String -Pattern '\[dependencies\]' -Context 0,30
```

- [ ] **Step 2: Add these five crates to `[dependencies]`**

Append, in alphabetical order, into the existing `[dependencies]` block of `src-tauri/Cargo.toml`:

```toml
cpal = "0.15"
crossbeam-channel = "0.5"
hound = "3.5"
ort = { version = "2", default-features = false, features = ["download-binaries"] }
voice_activity_detector = "0.2"
```

Notes for the executor:
- `ort` declared explicitly (rather than relying on the transitive dep through `voice_activity_detector`) so we control the `download-binaries` feature flag. Without that flag, ort fails to find the ONNX Runtime library at runtime.
- If the build fails because `ort` cannot reach its binary release host (rare but possible on a sandboxed CI runner), report **BLOCKED** — fallback would be to load a system-installed ONNX Runtime DLL, which we don't want to take on yet.

- [ ] **Step 3: Run `cargo check` to fetch and compile the new deps**

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: success. First compile is slow (cpal pulls Windows API crates; ort downloads a ~50 MB ONNX Runtime archive on the first build). On a warm machine this should be 2–5 minutes; on a cold machine 10–15.

- [ ] **Step 4: Verify the existing 13 Rust tests still pass**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: `test result: ok. 13 passed`.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/Cargo.toml Cargo.lock
git commit -m "chore(deps): add cpal, voice_activity_detector, ort, hound, crossbeam-channel"
```

---

## Task 2: Extend `paths.rs` with `audio_dir()`

**Files:**
- Modify: `src-tauri/src/paths.rs`

- [ ] **Step 1: Read the current file**

The current `src-tauri/src/paths.rs` (after T7's BaseDirs fix) defines `app_data_dir()` returning `%APPDATA%\voicetabs\` and `log_dir()` returning the `logs/` subdirectory.

- [ ] **Step 2: Add `audio_dir()` immediately after `log_dir()`**

Append this function to `src-tauri/src/paths.rs`:

```rust
pub fn audio_dir() -> anyhow::Result<PathBuf> {
    let dir = app_data_dir()?.join("audio");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
```

- [ ] **Step 3: Run `cargo build` to confirm it compiles**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

Expected: clean compile.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/paths.rs
git commit -m "feat(paths): add audio_dir() for utterance WAV output"
```

---

## Task 3: Audio device enumeration

**Files:**
- Create: `src-tauri/src/audio/mod.rs`
- Create: `src-tauri/src/audio/devices.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod audio`)

- [ ] **Step 1: Create `src-tauri/src/audio/mod.rs`**

```rust
pub mod devices;
pub mod input;

pub use devices::{default_input_name, list_input_devices};
pub use input::{spawn_input_stream, AudioConfig, InputStreamHandle};
```

(The `input` submodule is added in Task 4; declare it now so the module tree is final.)

- [ ] **Step 2: Create `src-tauri/src/audio/devices.rs`**

```rust
use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Debug, Clone, serde::Serialize)]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
}

/// Enumerate available input devices on the default host.
///
/// Returns an empty Vec if cpal cannot reach the host (extremely unusual on
/// Windows). The default device, if any, will be marked.
pub fn list_input_devices() -> Vec<InputDevice> {
    let host = cpal::default_host();
    let default = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    match host.input_devices() {
        Ok(iter) => iter
            .filter_map(|d| d.name().ok())
            .map(|name| InputDevice {
                is_default: name == default,
                name,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Returns the system default input device's name, or `None` if there is no
/// default (e.g. no microphone attached).
pub fn default_input_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.name().ok())
}
```

- [ ] **Step 3: Declare the module in `src-tauri/src/lib.rs`**

Add `pub mod audio;` to the module declarations near the top of `src-tauri/src/lib.rs`. Insert alphabetically; the existing declarations are `commands`, `db`, `logging`, `paths`. After insertion:

```rust
pub mod audio;
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;
```

(Do not change `run()` yet — Task 10 wires the controller into `setup()`.)

- [ ] **Step 4: Add a smoke test for device enumeration**

Create `src-tauri/tests/audio_devices_test.rs`:

```rust
//! Smoke test for cpal device enumeration. Does not assert on the contents
//! (CI runners may have no audio devices), only that the function returns
//! without panicking and produces well-formed structs.

#[test]
fn list_input_devices_does_not_panic() {
    let devices = voicetabs_lib::audio::list_input_devices();
    for d in &devices {
        assert!(!d.name.is_empty(), "device name must be non-empty");
    }
    // `is_default == true` for at most one device.
    let default_count = devices.iter().filter(|d| d.is_default).count();
    assert!(default_count <= 1, "got {default_count} defaults");
}

#[test]
fn default_input_name_does_not_panic() {
    let _ = voicetabs_lib::audio::default_input_name();
}
```

- [ ] **Step 5: Run the smoke test**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test audio_devices_test
```

Expected: 2 passed.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/audio src-tauri/src/lib.rs src-tauri/tests/audio_devices_test.rs
git commit -m "feat(audio): enumerate cpal input devices"
```

---

## Task 4: cpal capture into a crossbeam channel

**Files:**
- Create: `src-tauri/src/audio/input.rs`

The cpal `Stream` is `!Send` on Windows, so it must live on the thread that built it. The pattern: a builder function returns a `Stream` plus a `Receiver<Vec<f32>>`. The caller is expected to hold the `Stream` on a specific thread (the capture worker thread in Task 10) and read frames from the receiver on the same thread.

- [ ] **Step 1: Create `src-tauri/src/audio/input.rs`**

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamConfig};
use crossbeam_channel::{bounded, Receiver, Sender};

/// Target audio configuration. We always request mono 16 kHz f32.
/// WASAPI shared mode handles the device-rate → 16 kHz resampling for us.
#[derive(Debug, Clone, Copy)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self { sample_rate: 16_000, channels: 1 }
    }
}

/// Holds the cpal Stream (which must NOT be dropped while we want audio) and
/// the receiver side of the sample channel. The Stream is `!Send` — keep this
/// struct on the thread that built it.
pub struct InputStreamHandle {
    _stream: Stream,
    pub frames: Receiver<Vec<f32>>,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no default input device available")]
    NoDevice,
    #[error("cpal error: {0}")]
    Cpal(String),
}

/// Build a 16 kHz mono f32 input stream from the system default input device.
///
/// The returned handle owns the `Stream`. As long as the handle is alive and
/// `_stream` is not dropped, audio frames are pushed into `frames`. Dropping
/// the handle stops capture cleanly.
///
/// `channel_capacity` is the upper bound on queued frames; if the consumer
/// stalls, oldest frames are dropped (callback uses `try_send`).
pub fn spawn_input_stream(
    cfg: AudioConfig,
    channel_capacity: usize,
) -> Result<InputStreamHandle, AudioError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(AudioError::NoDevice)?;

    let stream_cfg = StreamConfig {
        channels: cfg.channels,
        sample_rate: SampleRate(cfg.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let (tx, rx) = bounded::<Vec<f32>>(channel_capacity);

    // We always request f32 samples. cpal converts from the device's native
    // format. If conversion fails for some exotic device, `build_input_stream`
    // returns an error and we surface it.
    let supported_format = SampleFormat::F32;

    let err_fn = |e| tracing::warn!("cpal input stream error: {e}");

    let stream = match supported_format {
        SampleFormat::F32 => device
            .build_input_stream(
                &stream_cfg,
                {
                    let tx: Sender<Vec<f32>> = tx;
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        // `data` is borrowed; we must copy before sending.
                        // bounded::try_send drops the frame if the queue is
                        // full — preferable to blocking the audio thread.
                        let frame = data.to_vec();
                        let _ = tx.try_send(frame);
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| AudioError::Cpal(e.to_string()))?,
        _ => return Err(AudioError::Cpal(format!("unsupported sample format: {supported_format:?}"))),
    };

    stream
        .play()
        .map_err(|e| AudioError::Cpal(e.to_string()))?;

    Ok(InputStreamHandle { _stream: stream, frames: rx })
}
```

- [ ] **Step 2: Verify it compiles**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

Expected: clean compile. There may be cpal warnings about unused fields on Linux/macOS feature paths — those are not our concern on Windows-only v1.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/audio/input.rs
git commit -m "feat(audio): cpal input stream → crossbeam frame channel"
```

Note: a runtime test of this code requires a real audio device and is impractical in `cargo test`. The full capture pipeline is exercised in the manual acceptance in Task 16.

---

## Task 5: VAD model wrapper

**Files:**
- Create: `src-tauri/src/vad/mod.rs`
- Create: `src-tauri/src/vad/model.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod vad`)

- [ ] **Step 1: Create `src-tauri/src/vad/mod.rs`**

```rust
pub mod model;
pub mod state;

pub use model::{VadModel, VadModelError, CHUNK_SAMPLES};
pub use state::{VadEvent, VadState, VadStateMachine};
```

(The `state` submodule is added in Task 6; declare it now.)

- [ ] **Step 2: Create `src-tauri/src/vad/model.rs`**

```rust
use voice_activity_detector::VoiceActivityDetector;

/// Number of f32 samples per VAD prediction at 16 kHz. 512 samples ≈ 32 ms,
/// which is one of the chunk sizes Silero v5 was trained for.
pub const CHUNK_SAMPLES: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum VadModelError {
    #[error("failed to load Silero VAD model: {0}")]
    Build(String),
    #[error("wrong chunk size: expected {expected}, got {got}")]
    ChunkSize { expected: usize, got: usize },
}

/// Thin wrapper around the Silero VAD model. Holds LSTM state internally;
/// every call to `predict` updates the state, so the wrapper is mutable and
/// must not be shared across threads.
pub struct VadModel {
    inner: VoiceActivityDetector,
}

impl VadModel {
    pub fn new() -> Result<Self, VadModelError> {
        let inner = VoiceActivityDetector::builder()
            .sample_rate(16_000_i64)
            .chunk_size(CHUNK_SAMPLES)
            .build()
            .map_err(|e| VadModelError::Build(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Run inference on exactly `CHUNK_SAMPLES` samples. Returns p(speech) in
    /// the range [0.0, 1.0].
    pub fn predict(&mut self, samples: &[f32]) -> Result<f32, VadModelError> {
        if samples.len() != CHUNK_SAMPLES {
            return Err(VadModelError::ChunkSize {
                expected: CHUNK_SAMPLES,
                got: samples.len(),
            });
        }
        Ok(self.inner.predict(samples.iter().copied()))
    }
}
```

- [ ] **Step 3: Declare the module in `src-tauri/src/lib.rs`**

Update the module list:

```rust
pub mod audio;
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;
pub mod vad;
```

- [ ] **Step 4: Add a smoke test that the model loads and accepts a chunk**

Create `src-tauri/tests/vad_model_test.rs`:

```rust
use voicetabs_lib::vad::{VadModel, CHUNK_SAMPLES};

#[test]
fn model_loads_and_predicts_silence_low() {
    let mut m = VadModel::new().expect("should build Silero model");
    let silence = vec![0.0_f32; CHUNK_SAMPLES];
    let p = m.predict(&silence).expect("predict ok");
    assert!((0.0..=1.0).contains(&p), "prob out of range: {p}");
    // Silence should be confidently classified as non-speech.
    assert!(p < 0.3, "silence should score low; got {p}");
}

#[test]
fn predict_rejects_wrong_chunk_size() {
    let mut m = VadModel::new().expect("should build Silero model");
    let too_short = vec![0.0_f32; 100];
    assert!(m.predict(&too_short).is_err());
}
```

- [ ] **Step 5: Run the smoke test**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test vad_model_test
```

Expected: 2 passed. **The first run is slow** — `ort` materializes the bundled ONNX Runtime; subsequent runs are fast.

If `ort` reports it cannot find the ONNX Runtime library at test time, the most likely cause is that `download-binaries` was not enabled in Task 1. Re-check the `Cargo.toml` entry. If that's correct and it still fails, report **BLOCKED** so we can investigate (e.g. switch to ort's `load-dynamic` mode and ship a DLL).

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/vad src-tauri/src/lib.rs src-tauri/tests/vad_model_test.rs
git commit -m "feat(vad): wrap Silero VAD via voice_activity_detector"
```

---

## Task 6: VAD state machine (TDD)

The model gives a per-chunk probability. The state machine layers hysteresis on top: rising edge requires sustained probability ≥ `0.5` for ≥ 120 ms; falling edge requires sustained probability ≤ `0.35` for ≥ 700 ms. Brief noise bursts and brief pauses don't flip the state.

**Files:**
- Create: `src-tauri/src/vad/state.rs`

- [ ] **Step 1: Write the tests first**

Create `src-tauri/src/vad/state.rs` with the test module already present. We'll add the implementation in Step 2.

```rust
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadState {
    Idle,
    Speaking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    RisingEdge { timestamp_ms: u64 },
    FallingEdge { timestamp_ms: u64 },
}

/// VAD hysteresis state machine. One instance per pipeline.
///
/// Defaults match spec §5.2: rising threshold 0.5 sustained 120 ms;
/// falling threshold 0.35 sustained 700 ms. `chunk_ms` is the duration of one
/// VAD prediction (32 ms at 16 kHz with 512-sample chunks).
pub struct VadStateMachine {
    state: VadState,
    rising_threshold: f32,
    falling_threshold: f32,
    rising_required_chunks: u32,
    falling_required_chunks: u32,
    above_count: u32,
    below_count: u32,
}

impl VadStateMachine {
    pub fn new(chunk_ms: u32) -> Self {
        let chunk_ms = chunk_ms.max(1);
        Self {
            state: VadState::Idle,
            rising_threshold: 0.5,
            falling_threshold: 0.35,
            rising_required_chunks: 120 / chunk_ms,
            falling_required_chunks: 700 / chunk_ms,
            above_count: 0,
            below_count: 0,
        }
    }

    pub fn state(&self) -> VadState {
        self.state
    }

    /// Feed one probability + its timestamp. Returns a `VadEvent` if the state
    /// transitioned, `None` otherwise.
    pub fn observe(&mut self, prob: f32, timestamp_ms: u64) -> Option<VadEvent> {
        match self.state {
            VadState::Idle => {
                if prob.partial_cmp(&self.rising_threshold) != Some(Ordering::Less) {
                    self.above_count += 1;
                    if self.above_count >= self.rising_required_chunks {
                        self.state = VadState::Speaking;
                        self.above_count = 0;
                        self.below_count = 0;
                        return Some(VadEvent::RisingEdge { timestamp_ms });
                    }
                } else {
                    self.above_count = 0;
                }
            }
            VadState::Speaking => {
                if prob.partial_cmp(&self.falling_threshold) != Some(Ordering::Greater) {
                    self.below_count += 1;
                    if self.below_count >= self.falling_required_chunks {
                        self.state = VadState::Idle;
                        self.above_count = 0;
                        self.below_count = 0;
                        return Some(VadEvent::FallingEdge { timestamp_ms });
                    }
                } else {
                    self.below_count = 0;
                }
            }
        }
        None
    }

    /// Force the state machine back to Idle and clear counters. Used when the
    /// utterance builder force-closes an utterance at the max-duration cap.
    pub fn force_idle(&mut self) {
        self.state = VadState::Idle;
        self.above_count = 0;
        self.below_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chunk size used by every test: 32 ms (matches Silero at 16 kHz / 512).
    /// At 32 ms, rising needs 4 chunks (120/32 = 3.75 → 3), falling needs
    /// 21 chunks (700/32 = 21.875 → 21).
    const CHUNK_MS: u32 = 32;

    fn observe_n(sm: &mut VadStateMachine, prob: f32, n: u32) -> Vec<VadEvent> {
        let mut events = Vec::new();
        for i in 0..n {
            if let Some(e) = sm.observe(prob, (i as u64) * CHUNK_MS as u64) {
                events.push(e);
            }
        }
        events
    }

    #[test]
    fn idle_stays_idle_on_silence() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        let events = observe_n(&mut sm, 0.05, 1_000);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Idle);
    }

    #[test]
    fn sustained_speech_triggers_rising_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        let events = observe_n(&mut sm, 0.9, 10);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], VadEvent::RisingEdge { .. }));
        assert_eq!(sm.state(), VadState::Speaking);
    }

    #[test]
    fn brief_burst_does_not_trigger_rising_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        // 60 ms of "speech" (2 chunks) — below the 120 ms threshold.
        let events = observe_n(&mut sm, 0.9, 2);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Idle);
    }

    #[test]
    fn rising_then_sustained_silence_triggers_falling_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        // Trigger rising.
        observe_n(&mut sm, 0.9, 10);
        assert_eq!(sm.state(), VadState::Speaking);
        // 800 ms of silence (25 chunks).
        let events = observe_n(&mut sm, 0.05, 25);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], VadEvent::FallingEdge { .. }));
        assert_eq!(sm.state(), VadState::Idle);
    }

    #[test]
    fn brief_pause_does_not_trigger_falling_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        observe_n(&mut sm, 0.9, 10);
        assert_eq!(sm.state(), VadState::Speaking);
        // 300 ms of silence (~9 chunks) — below the 700 ms threshold.
        let events = observe_n(&mut sm, 0.05, 9);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Speaking);
    }

    #[test]
    fn hysteresis_resets_below_count_when_speech_returns() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        observe_n(&mut sm, 0.9, 10);
        observe_n(&mut sm, 0.05, 10); // ~320 ms of silence
        // Re-speech briefly; the below-count must reset.
        observe_n(&mut sm, 0.9, 2);
        // Now another short silence; should NOT immediately fire falling edge
        // because the silence streak restarted from zero.
        let events = observe_n(&mut sm, 0.05, 10);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Speaking);
    }

    #[test]
    fn force_idle_drops_back_with_no_event() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        observe_n(&mut sm, 0.9, 10);
        assert_eq!(sm.state(), VadState::Speaking);
        sm.force_idle();
        assert_eq!(sm.state(), VadState::Idle);
        // Same hysteresis applies after force_idle.
        let events = observe_n(&mut sm, 0.9, 2);
        assert!(events.is_empty(), "got {events:?}");
    }

    #[test]
    fn timestamp_is_passed_through_to_event() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        for i in 0..3 {
            assert!(sm.observe(0.9, i * 100).is_none());
        }
        let ev = sm.observe(0.9, 999).expect("rising edge");
        match ev {
            VadEvent::RisingEdge { timestamp_ms } => assert_eq!(timestamp_ms, 999),
            other => panic!("expected RisingEdge, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml vad::state
```

Expected: 8 tests passed.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/vad/state.rs
git commit -m "feat(vad): hysteresis state machine with rising/falling edge events"
```

---

## Task 7: WAV writer (TDD)

We write 16 kHz mono 16-bit PCM WAVs. Input is `&[f32]` in the range `[-1.0, 1.0]`; we clip and convert to `i16`. `hound` is the canonical WAV crate.

**Files:**
- Create: `src-tauri/src/utterance/mod.rs`
- Create: `src-tauri/src/utterance/wav.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod utterance`)

- [ ] **Step 1: Create `src-tauri/src/utterance/mod.rs`**

```rust
pub mod builder;
pub mod preroll;
pub mod wav;

pub use builder::UtteranceBuilder;
pub use preroll::PreRoll;
pub use wav::write_pcm16_wav;
```

(`builder` and `preroll` are added in later tasks; declare them now.)

- [ ] **Step 2: Write `src-tauri/src/utterance/wav.rs`**

```rust
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

/// Write a 16 kHz mono 16-bit PCM WAV from f32 samples in [-1.0, 1.0].
/// Samples outside the range are clipped.
pub fn write_pcm16_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> anyhow::Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)?;
    for &s in samples {
        let clipped = s.clamp(-1.0, 1.0);
        // Symmetric scale so 1.0 → 32767 and -1.0 → -32767. Avoids the
        // off-by-one at the negative extreme that you'd get with 32768.0.
        let v = (clipped * 32767.0) as i16;
        writer.write_sample(v)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;
    use tempfile::tempdir;

    #[test]
    fn round_trip_preserves_sample_rate_and_channel() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let samples: Vec<f32> = (0..16_000).map(|i| (i as f32 / 1000.0).sin()).collect();
        write_pcm16_wav(&path, 16_000, &samples).unwrap();

        let reader = WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);
    }

    #[test]
    fn clips_out_of_range_input() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let samples = vec![2.0_f32, -2.0, 0.5, -0.5];
        write_pcm16_wav(&path, 16_000, &samples).unwrap();

        let mut reader = WavReader::open(&path).unwrap();
        let read_back: Vec<i16> = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(read_back.len(), 4);
        assert_eq!(read_back[0], 32_767); // clipped from 2.0
        assert_eq!(read_back[1], -32_767); // clipped from -2.0
        assert!((read_back[2] - 16_383).abs() <= 1, "got {}", read_back[2]);
        assert!((read_back[3] + 16_383).abs() <= 1, "got {}", read_back[3]);
    }

    #[test]
    fn writes_an_empty_file_for_empty_input() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        write_pcm16_wav(&path, 16_000, &[]).unwrap();
        let reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.duration(), 0);
    }
}
```

- [ ] **Step 3: Declare the module in `src-tauri/src/lib.rs`**

```rust
pub mod audio;
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;
pub mod utterance;
pub mod vad;
```

- [ ] **Step 4: Run the WAV tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml utterance::wav
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/utterance src-tauri/src/lib.rs
git commit -m "feat(utterance): hound-based 16 kHz mono i16 WAV writer"
```

---

## Task 8: Pre-roll ring buffer (TDD)

The pre-roll holds the last N samples (configured as 500 ms × sample-rate = 8000 samples at 16 kHz). When VAD reports a rising edge, the pre-roll is drained and prepended to the utterance — this captures the ~200 ms of speech that fired below the threshold before the VAD became confident.

**Files:**
- Create: `src-tauri/src/utterance/preroll.rs`

- [ ] **Step 1: Write the file with tests**

```rust
use std::collections::VecDeque;

/// FIFO ring of recent samples. Push appends; when the buffer is full, the
/// oldest sample is evicted. `drain` returns and clears the contents.
pub struct PreRoll {
    deque: VecDeque<f32>,
    capacity: usize,
}

impl PreRoll {
    pub fn new(capacity_samples: usize) -> Self {
        Self {
            deque: VecDeque::with_capacity(capacity_samples),
            capacity: capacity_samples,
        }
    }

    /// Push samples; oldest evicted if over capacity.
    pub fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            if self.deque.len() == self.capacity {
                self.deque.pop_front();
            }
            self.deque.push_back(s);
        }
    }

    /// Drain the current contents into a `Vec<f32>` and clear the ring.
    pub fn drain(&mut self) -> Vec<f32> {
        self.deque.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.deque.len()
    }

    pub fn is_empty(&self) -> bool {
        self.deque.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_below_capacity_retains_all() {
        let mut p = PreRoll::new(10);
        p.push(&[1.0, 2.0, 3.0]);
        assert_eq!(p.len(), 3);
        let drained = p.drain();
        assert_eq!(drained, vec![1.0, 2.0, 3.0]);
        assert!(p.is_empty());
    }

    #[test]
    fn push_above_capacity_evicts_oldest() {
        let mut p = PreRoll::new(3);
        p.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(p.len(), 3);
        let drained = p.drain();
        assert_eq!(drained, vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn drain_clears_buffer() {
        let mut p = PreRoll::new(10);
        p.push(&[1.0, 2.0]);
        let _ = p.drain();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn zero_capacity_never_retains() {
        let mut p = PreRoll::new(0);
        p.push(&[1.0, 2.0, 3.0]);
        assert!(p.is_empty());
        let drained = p.drain();
        assert!(drained.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml utterance::preroll
```

Expected: 4 passed.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/utterance/preroll.rs
git commit -m "feat(utterance): preroll ring buffer for VAD edge backfill"
```

---

## Task 9: Utterance builder (TDD)

The builder coordinates pre-roll + accumulation + WAV writing. It does NOT load files itself — `write_pcm16_wav` is injected via the path-output callback, so the unit test can run on a tempdir.

State machine:

- **Idle**: incoming frames go into the pre-roll. On `RisingEdge`, the pre-roll is drained and becomes the seed of a new utterance; transition to Recording.
- **Recording**: incoming frames are appended to the current utterance. On `FallingEdge`, finalize → return WAV path. If the utterance exceeds 30 s, finalize early and transition back to Idle (the next frame, if speech, will trigger a new utterance via the state machine — but in this Phase 2 we let the state-machine reset only when the builder force-closes; we re-emit a synthetic rising edge in the controller layer in Task 10).

Spec note: A "force close at 30 s cap" plus "user is still talking" produces two adjacent utterances. The seam is acceptable per spec §12: "Long monologues exceeding the 30 s cap split into multiple segments mid-sentence".

**Files:**
- Create: `src-tauri/src/utterance/builder.rs`

- [ ] **Step 1: Write the file with tests**

```rust
use std::path::PathBuf;

use crate::utterance::{preroll::PreRoll, wav::write_pcm16_wav};
use crate::vad::VadEvent;

/// Configuration for the builder.
#[derive(Debug, Clone, Copy)]
pub struct UtteranceConfig {
    pub sample_rate: u32,
    pub pre_roll_ms: u32,
    pub max_utterance_ms: u32,
}

impl Default for UtteranceConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            pre_roll_ms: 500,
            max_utterance_ms: 30_000,
        }
    }
}

/// The currently in-progress utterance, if any.
struct Current {
    started_at_ms: u64,
    samples: Vec<f32>,
}

/// Builds utterance WAV files from a stream of audio frames + VAD events.
///
/// The builder is a pure data-flow component: it does not own the audio
/// thread, nor any I/O loop. The caller drives it via `push_frame` and
/// `on_vad_event`. When `on_vad_event` returns `Some(path)` or `push_frame`
/// triggers a max-cap finalization that returns `Some(path)`, a WAV file has
/// been written.
pub struct UtteranceBuilder {
    cfg: UtteranceConfig,
    pre_roll: PreRoll,
    current: Option<Current>,
    output_dir: PathBuf,
    max_samples: usize,
}

impl UtteranceBuilder {
    pub fn new(cfg: UtteranceConfig, output_dir: PathBuf) -> Self {
        let pre_roll_samples =
            (cfg.sample_rate as usize * cfg.pre_roll_ms as usize) / 1000;
        let max_samples =
            (cfg.sample_rate as usize * cfg.max_utterance_ms as usize) / 1000;
        Self {
            cfg,
            pre_roll: PreRoll::new(pre_roll_samples),
            current: None,
            output_dir,
            max_samples,
        }
    }

    /// Append audio samples. If we're currently recording, they go straight
    /// into the utterance and the max-duration cap is checked. Otherwise they
    /// go into the pre-roll ring.
    ///
    /// Returns `Some(path)` if the max-duration cap was hit and a WAV was
    /// emitted as a result.
    pub fn push_frame(&mut self, samples: &[f32]) -> Option<PathBuf> {
        if let Some(current) = &mut self.current {
            current.samples.extend_from_slice(samples);
            if current.samples.len() >= self.max_samples {
                return self.finalize();
            }
            None
        } else {
            self.pre_roll.push(samples);
            None
        }
    }

    /// Handle a VAD edge event. Returns `Some(path)` if a WAV was emitted as
    /// a result (only on FallingEdge or max-cap finalization).
    pub fn on_vad_event(&mut self, event: VadEvent) -> Option<PathBuf> {
        match event {
            VadEvent::RisingEdge { timestamp_ms } => {
                let mut samples = self.pre_roll.drain();
                // Reserve a moderate chunk so common short utterances don't
                // reallocate the Vec repeatedly.
                samples.reserve(self.cfg.sample_rate as usize * 2);
                self.current = Some(Current {
                    started_at_ms: timestamp_ms,
                    samples,
                });
                None
            }
            VadEvent::FallingEdge { .. } => self.finalize(),
        }
    }

    fn finalize(&mut self) -> Option<PathBuf> {
        let current = self.current.take()?;
        if current.samples.is_empty() {
            return None;
        }
        let path = self
            .output_dir
            .join(format!("{}.wav", current.started_at_ms));
        match write_pcm16_wav(&path, self.cfg.sample_rate, &current.samples) {
            Ok(()) => Some(path),
            Err(e) => {
                tracing::error!("failed to write utterance WAV {path:?}: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;
    use tempfile::tempdir;

    fn ones(n: usize) -> Vec<f32> {
        vec![0.5_f32; n]
    }

    fn cfg() -> UtteranceConfig {
        UtteranceConfig {
            sample_rate: 16_000,
            pre_roll_ms: 500,
            max_utterance_ms: 30_000,
        }
    }

    #[test]
    fn idle_frames_go_into_pre_roll() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());
        // Push 320 samples (20 ms). Idle → into pre-roll.
        let path = b.push_frame(&ones(320));
        assert!(path.is_none());
        // No file yet.
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn rising_edge_drains_pre_roll_into_current() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());
        b.push_frame(&ones(1_000));
        let result = b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 42 });
        assert!(result.is_none(), "rising edge does not write a file");
        // After rising, more frames go into the current utterance.
        b.push_frame(&ones(500));
        let path = b
            .on_vad_event(VadEvent::FallingEdge { timestamp_ms: 100 })
            .expect("falling edge writes WAV");
        // The filename uses the rising-edge timestamp.
        assert!(path.to_string_lossy().ends_with("42.wav"));
        // The WAV contains pre-roll (1000) + recorded (500) = 1500 samples.
        let reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.duration() as usize, 1_500);
    }

    #[test]
    fn falling_edge_without_rising_is_a_noop() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());
        let path = b.on_vad_event(VadEvent::FallingEdge { timestamp_ms: 0 });
        assert!(path.is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn max_duration_cap_force_finalizes() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(
            UtteranceConfig {
                sample_rate: 16_000,
                pre_roll_ms: 0,
                max_utterance_ms: 1_000, // tiny cap so we hit it fast
            },
            dir.path().to_path_buf(),
        );
        b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 7 });
        // 16 000 samples per second × 1 s cap = 16 000 samples; one frame puts
        // us over.
        let result = b.push_frame(&ones(20_000));
        let path = result.expect("max cap should finalize");
        assert!(path.to_string_lossy().ends_with("7.wav"));
    }

    #[test]
    fn two_utterances_produce_two_files() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());

        b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 1 });
        b.push_frame(&ones(800));
        let first = b
            .on_vad_event(VadEvent::FallingEdge { timestamp_ms: 50 })
            .expect("first WAV");
        assert!(first.to_string_lossy().ends_with("1.wav"));

        b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 100 });
        b.push_frame(&ones(800));
        let second = b
            .on_vad_event(VadEvent::FallingEdge { timestamp_ms: 200 })
            .expect("second WAV");
        assert!(second.to_string_lossy().ends_with("100.wav"));

        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(files.len(), 2);
    }
}
```

- [ ] **Step 2: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml utterance::builder
```

Expected: 5 passed.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/utterance/builder.rs
git commit -m "feat(utterance): builder coordinating preroll + VAD edges + WAV write"
```

---

## Task 10: Capture controller

The controller owns the worker thread that holds the `!Send` cpal `Stream`. The thread services `CaptureCmd` messages (Start / Stop) over one channel and drains audio frames over another. Inside the worker, samples are chunked into 512-sample windows and fed to the VAD model + state machine + utterance builder.

The controller exposes a `Send + Sync` API to the rest of the app: a method to send commands and a method to read the current status. It is held inside Tauri's `manage()` slot.

**Files:**
- Create: `src-tauri/src/capture/mod.rs`
- Create: `src-tauri/src/capture/controller.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod capture`)

- [ ] **Step 1: Create `src-tauri/src/capture/mod.rs`**

```rust
pub mod controller;

pub use controller::{CaptureController, CaptureStatus};
```

- [ ] **Step 2: Create `src-tauri/src/capture/controller.rs`**

```rust
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::{select, unbounded, Receiver, Sender};
use parking_lot::Mutex;
use serde::Serialize;

use crate::audio::{spawn_input_stream, AudioConfig, InputStreamHandle};
use crate::utterance::{builder::UtteranceConfig, UtteranceBuilder};
use crate::vad::{VadModel, VadStateMachine, CHUNK_SAMPLES};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptureStatus {
    Idle,
    Capturing { device_name: Option<String> },
    Error { message: String },
}

enum Cmd {
    Start,
    Stop,
}

/// Public capture controller, held inside Tauri's `manage()` slot.
/// `Send + Sync` — the `Stream` itself lives only on the worker thread.
pub struct CaptureController {
    cmd_tx: Sender<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
}

impl CaptureController {
    /// Spawn the worker thread and return the controller handle. The worker
    /// runs for the lifetime of the process; we never join it.
    pub fn spawn(output_dir: PathBuf) -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let status = Arc::new(Mutex::new(CaptureStatus::Idle));
        let status_for_worker = status.clone();
        std::thread::Builder::new()
            .name("voicetabs-capture".into())
            .spawn(move || worker_loop(cmd_rx, status_for_worker, output_dir))
            .expect("spawn capture thread");
        Self { cmd_tx, status }
    }

    pub fn start(&self) {
        let _ = self.cmd_tx.send(Cmd::Start);
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(Cmd::Stop);
    }

    pub fn status(&self) -> CaptureStatus {
        self.status.lock().clone()
    }
}

fn worker_loop(
    cmd_rx: Receiver<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    output_dir: PathBuf,
) {
    let mut stream_handle: Option<InputStreamHandle> = None;
    let mut vad_model: Option<VadModel> = None;
    let mut vad_sm = VadStateMachine::new(32);
    let mut builder = UtteranceBuilder::new(UtteranceConfig::default(), output_dir);
    let mut accumulator: Vec<f32> = Vec::with_capacity(CHUNK_SAMPLES * 2);

    loop {
        // Clone the frames receiver out of the optional handle BEFORE the
        // select! so the borrow on `stream_handle` ends at this statement.
        // That lets the Stop arm reassign `stream_handle = None;` cleanly.
        let frames_rx = stream_handle.as_ref().map(|h| h.frames.clone());

        if let Some(frames_rx) = frames_rx {
            select! {
                recv(cmd_rx) -> cmd => {
                    match cmd {
                        Ok(Cmd::Start) => { /* already capturing */ }
                        Ok(Cmd::Stop) => {
                            stream_handle = None;
                            vad_sm.force_idle();
                            accumulator.clear();
                            *status.lock() = CaptureStatus::Idle;
                        }
                        Err(_) => return,
                    }
                }
                recv(frames_rx) -> frame => {
                    let Ok(frame) = frame else { continue };
                    // Lazy-load the VAD model the first time we need it.
                    if vad_model.is_none() {
                        match VadModel::new() {
                            Ok(m) => vad_model = Some(m),
                            Err(e) => {
                                tracing::error!("VAD model load failed: {e}");
                                *status.lock() = CaptureStatus::Error {
                                    message: format!("VAD load failed: {e}"),
                                };
                                stream_handle = None;
                                continue;
                            }
                        }
                    }
                    let model = vad_model.as_mut().expect("model loaded above");
                    accumulator.extend_from_slice(&frame);
                    process_chunks(&mut accumulator, model, &mut vad_sm, &mut builder);
                }
            }
        } else {
            // Idle: block on commands only.
            match cmd_rx.recv() {
                Ok(Cmd::Start) => match spawn_input_stream(AudioConfig::default(), 64) {
                    Ok(handle) => {
                        let device_name = crate::audio::default_input_name();
                        *status.lock() = CaptureStatus::Capturing { device_name };
                        stream_handle = Some(handle);
                    }
                    Err(e) => {
                        tracing::error!("capture start failed: {e}");
                        *status.lock() = CaptureStatus::Error {
                            message: e.to_string(),
                        };
                    }
                },
                Ok(Cmd::Stop) => { /* already stopped */ }
                Err(_) => return,
            }
        }
    }
}

/// Drain `accumulator` in 512-sample windows, running VAD + state machine +
/// utterance builder for each window. Leaves any remainder in place.
fn process_chunks(
    accumulator: &mut Vec<f32>,
    model: &mut VadModel,
    sm: &mut VadStateMachine,
    builder: &mut UtteranceBuilder,
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
            // Push the chunk BEFORE handling the event:
            //   - On RisingEdge: the chunk lands in the pre-roll first, then
            //     the event drains the pre-roll into the new current
            //     utterance, so the triggering chunk is included.
            //   - On FallingEdge: the chunk is appended to the current
            //     utterance first; the event finalizes including this chunk.
            let _ = builder.push_frame(&chunk);
            if let Some(path) = builder.on_vad_event(event) {
                tracing::info!("wrote utterance WAV: {path:?}");
            }
        } else if let Some(path) = builder.push_frame(&chunk) {
            // No edge event but the max-duration cap finalized a WAV.
            tracing::info!("wrote (max-cap) utterance WAV: {path:?}");
            sm.force_idle();
        }
    }
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
```

- [ ] **Step 3: Declare `pub mod capture;` in `src-tauri/src/lib.rs`**

```rust
pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;
pub mod utterance;
pub mod vad;
```

- [ ] **Step 4: Run `cargo build` to confirm it compiles**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

Expected: clean compile. If `crossbeam_channel::select!` macro reports an issue with the `recv(frames_rx) -> frame` pattern, double-check the `crossbeam-channel` version. The macro is documented for ≥ 0.5; if a version mismatch surfaces, escalate.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/capture src-tauri/src/lib.rs
git commit -m "feat(capture): CaptureController + worker thread orchestrating audio+VAD+utterance"
```

---

## Task 11: Tauri commands for capture

**Files:**
- Create: `src-tauri/src/commands/capture.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs` (add `manage(controller)`, register handlers)

- [ ] **Step 1: Write `src-tauri/src/commands/capture.rs`**

```rust
use tauri::State;

use crate::capture::{CaptureController, CaptureStatus};

use super::tabs::CommandError;

#[tauri::command]
pub fn capture_start(controller: State<'_, CaptureController>) -> Result<(), CommandError> {
    controller.start();
    Ok(())
}

#[tauri::command]
pub fn capture_stop(controller: State<'_, CaptureController>) -> Result<(), CommandError> {
    controller.stop();
    Ok(())
}

#[tauri::command]
pub fn capture_status(controller: State<'_, CaptureController>) -> Result<CaptureStatus, CommandError> {
    Ok(controller.status())
}
```

- [ ] **Step 2: Update `src-tauri/src/commands/mod.rs`**

```rust
pub mod capture;
pub mod settings;
pub mod tabs;
```

- [ ] **Step 3: Update `src-tauri/src/lib.rs`** to spawn the controller and register handlers.

Current `run()` is:

```rust
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
}
```

Replace with:

```rust
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
```

- [ ] **Step 4: Run all tests to make sure nothing regressed**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all prior tests still pass (logging 1 + db::connection 2 + db::tabs 6 + db::settings 4 + audio_devices 2 + vad_model 2 + vad::state 8 + utterance::wav 3 + utterance::preroll 4 + utterance::builder 5 = **37 tests**).

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/commands src-tauri/src/lib.rs
git commit -m "feat(commands): capture_start / capture_stop / capture_status"
```

---

## Task 12: Frontend Tauri wrappers + capture store

**Files:**
- Modify: `src/lib/tauri.ts` (append captureApi)
- Create: `src/stores/captureStore.ts`

- [ ] **Step 1: Append to `src/lib/tauri.ts`**

Add this block at the end of the file, after `settingsApi`:

```ts
export type CaptureStatus =
  | { state: "idle" }
  | { state: "capturing"; device_name: string | null }
  | { state: "error"; message: string };

export const captureApi = {
  start(): Promise<void> {
    return invoke<void>("capture_start");
  },
  stop(): Promise<void> {
    return invoke<void>("capture_stop");
  },
  status(): Promise<CaptureStatus> {
    return invoke<CaptureStatus>("capture_status");
  },
};
```

- [ ] **Step 2: Write `src/stores/captureStore.ts`**

```ts
import { create } from "zustand";

import { captureApi, CaptureStatus } from "../lib/tauri";

type CaptureState = {
  status: CaptureStatus;
  /** Polling timer handle for jsdom-friendly teardown in tests. */
  pollHandle: ReturnType<typeof setInterval> | null;

  refresh: () => Promise<void>;
  startCapture: () => Promise<void>;
  stopCapture: () => Promise<void>;
  startPolling: () => void;
  stopPolling: () => void;
};

const POLL_INTERVAL_MS = 1000;

export const useCaptureStore = create<CaptureState>((set, get) => ({
  status: { state: "idle" },
  pollHandle: null,

  async refresh() {
    try {
      const status = await captureApi.status();
      set({ status });
    } catch (e) {
      set({ status: { state: "error", message: String(e) } });
    }
  },

  async startCapture() {
    await captureApi.start();
    await get().refresh();
  },

  async stopCapture() {
    await captureApi.stop();
    await get().refresh();
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

- [ ] **Step 3: Run type-check + tests**

```powershell
npx tsc --noEmit
npm test
```

Expected: type-check clean; 8 tests pass.

- [ ] **Step 4: Commit**

```powershell
git add src/lib/tauri.ts src/stores/captureStore.ts
git commit -m "feat(frontend): captureApi wrapper + Zustand store"
```

---

## Task 13: i18n strings for capture controls

**Files:**
- Modify: `src/i18n/locales/pt-BR.json`
- Modify: `src/i18n/locales/en.json`

- [ ] **Step 1: Append the `capture` block to `src/i18n/locales/pt-BR.json`**

The existing `capture` block currently has `modeAlwaysOn` and `modePtt`. Replace it with the expanded version:

```json
  "capture": {
    "modeAlwaysOn": "Sempre ativo",
    "modePtt": "Push-to-talk",
    "toggleOff": "🎤 Capturar: OFF",
    "toggleOn": "🎤 Capturando",
    "starting": "Iniciando…",
    "errorPrefix": "Erro:"
  },
```

- [ ] **Step 2: Same for `src/i18n/locales/en.json`**

```json
  "capture": {
    "modeAlwaysOn": "Always on",
    "modePtt": "Push-to-talk",
    "toggleOff": "🎤 Capture: OFF",
    "toggleOn": "🎤 Capturing",
    "starting": "Starting…",
    "errorPrefix": "Error:"
  },
```

- [ ] **Step 3: Run `npm test` to make sure the existing i18n tests still pass**

```powershell
npm test
```

Expected: 8 pass. (No assertions on the new keys yet — Task 14 adds component tests.)

- [ ] **Step 4: Commit**

```powershell
git add src/i18n/locales
git commit -m "i18n(capture): add toggle / starting / error keys"
```

---

## Task 14: CaptureToggle component (TDD)

**Files:**
- Create: `src/components/CaptureToggle.tsx`
- Create: `src/__tests__/CaptureToggle.test.tsx`

- [ ] **Step 1: Write the failing test in `src/__tests__/CaptureToggle.test.tsx`**

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CaptureToggle } from "../components/CaptureToggle";
import { CaptureStatus } from "../lib/tauri";

describe("CaptureToggle", () => {
  it("renders OFF state with the off label", () => {
    render(
      <CaptureToggle
        status={{ state: "idle" }}
        onStart={() => {}}
        onStop={() => {}}
      />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/capture: off/i);
  });

  it("renders ON state with the capturing label", () => {
    const status: CaptureStatus = { state: "capturing", device_name: "Mic" };
    render(
      <CaptureToggle status={status} onStart={() => {}} onStop={() => {}} />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/capturing/i);
  });

  it("calls onStart when clicked while idle", () => {
    const onStart = vi.fn();
    render(
      <CaptureToggle
        status={{ state: "idle" }}
        onStart={onStart}
        onStop={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onStart).toHaveBeenCalled();
  });

  it("calls onStop when clicked while capturing", () => {
    const onStop = vi.fn();
    const status: CaptureStatus = { state: "capturing", device_name: null };
    render(
      <CaptureToggle status={status} onStart={() => {}} onStop={onStop} />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onStop).toHaveBeenCalled();
  });

  it("shows the error message when state is error", () => {
    const status: CaptureStatus = { state: "error", message: "no mic" };
    render(
      <CaptureToggle status={status} onStart={() => {}} onStop={() => {}} />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/no mic/i);
  });
});
```

- [ ] **Step 2: Run the tests to confirm failure**

```powershell
npm test -- src/__tests__/CaptureToggle.test.tsx
```

Expected: 5 failing tests, all complaining the import resolves to undefined.

- [ ] **Step 3: Write `src/components/CaptureToggle.tsx`**

```tsx
import { useTranslation } from "react-i18next";

import { CaptureStatus } from "../lib/tauri";

type Props = {
  status: CaptureStatus;
  onStart: () => void;
  onStop: () => void;
};

export function CaptureToggle({ status, onStart, onStop }: Props) {
  const { t } = useTranslation();

  if (status.state === "error") {
    return (
      <button
        className="footer-button capture-toggle capture-toggle--error"
        onClick={onStart}
        title={status.message}
      >
        {t("capture.errorPrefix")} {status.message}
      </button>
    );
  }

  const isOn = status.state === "capturing";
  return (
    <button
      className={`footer-button capture-toggle${isOn ? " capture-toggle--on" : ""}`}
      onClick={isOn ? onStop : onStart}
    >
      {isOn ? t("capture.toggleOn") : t("capture.toggleOff")}
    </button>
  );
}
```

- [ ] **Step 4: Add a small accent style for the ON state**

Append to `src/styles.css`:

```css

.capture-toggle--on {
  color: #5cd97c;
}

.capture-toggle--error {
  color: #ff6b6b;
}
```

- [ ] **Step 5: Run the tests again**

```powershell
npm test
```

Expected: all tests pass (8 existing + 5 new = 13).

- [ ] **Step 6: Commit**

```powershell
git add src/components/CaptureToggle.tsx src/__tests__/CaptureToggle.test.tsx src/styles.css
git commit -m "feat(ui): CaptureToggle footer button with idle/capturing/error states"
```

---

## Task 15: Wire the toggle into the App footer

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/__tests__/i18n.test.tsx` (mock new capture commands)

- [ ] **Step 1: Update `src/__tests__/i18n.test.tsx`**

The existing mock returns `null` for `settings_get` and a single tab for `tabs_list`. Now we also need to handle the three capture commands. Update the `vi.mock` block:

```tsx
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "tabs_list") {
      return [
        {
          id: 1,
          title: "Test",
          order_idx: 0,
          created_at: 0,
          updated_at: 0,
        },
      ];
    }
    if (command === "tabs_create") {
      const title = (args?.title as string) ?? "New";
      return { id: 2, title, order_idx: 1, created_at: 0, updated_at: 0 };
    }
    if (command === "settings_get") {
      return null;
    }
    if (command === "capture_status") {
      return { state: "idle" };
    }
    if (command === "capture_start" || command === "capture_stop") {
      return undefined;
    }
    // All other commands resolve to undefined / no-op.
    return undefined;
  }),
}));
```

Only the `vi.mock` block changes — leave the rest of the test file alone.

- [ ] **Step 2: Update `src/App.tsx`** to render the CaptureToggle in the footer and wire capture polling.

Current `src/App.tsx` (in summary): imports `TabStrip`, `SettingsDrawer`; uses `useTabsStore` and `useSettingsStore`; the footer has a `<span />` on the left and the settings gear on the right; document.title is synced via a useEffect.

Replace the file contents with:

```tsx
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { CaptureToggle } from "./components/CaptureToggle";
import { SettingsDrawer } from "./components/SettingsDrawer";
import { TabStrip } from "./components/TabStrip";
import { useCaptureStore } from "./stores/captureStore";
import { useSettingsStore } from "./stores/settingsStore";
import { useTabsStore } from "./stores/tabsStore";

export default function App() {
  const { t } = useTranslation();
  const tabs = useTabsStore();
  const settings = useSettingsStore();
  const capture = useCaptureStore();

  useEffect(() => {
    void (async () => {
      await settings.load();
      await tabs.load();
      await capture.refresh();
      capture.startPolling();
    })();
    return () => {
      capture.stopPolling();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    document.title = t("app.title");
  }, [t]);

  if (!tabs.loaded) {
    return <main className="app" />;
  }

  return (
    <main className="app">
      <TabStrip
        tabs={tabs.tabs}
        activeId={tabs.activeTabId}
        onSelect={(id) => void tabs.setActive(id)}
        onCreate={() => void tabs.createTab(t("tabs.newTabTitle"))}
        onRename={(id, title) => void tabs.renameTab(id, title)}
        onClose={(id) => void tabs.deleteTab(id)}
        onReorder={(ordered) => void tabs.reorderTabs(ordered)}
      />
      <div className="tab-body">
        {tabs.tabs.length > 0 && tabs.activeTabId != null && (
          <p style={{ padding: 16, color: "#888" }}>
            {tabs.tabs.find((t) => t.id === tabs.activeTabId)?.title}
          </p>
        )}
      </div>
      <footer className="app-footer">
        <CaptureToggle
          status={capture.status}
          onStart={() => void capture.startCapture()}
          onStop={() => void capture.stopCapture()}
        />
        <button onClick={settings.openDrawer} className="footer-button">
          ⚙ {t("settings.open")}
        </button>
      </footer>
      <SettingsDrawer />
    </main>
  );
}
```

- [ ] **Step 3: Run all tests**

```powershell
npm test
```

Expected: 13 tests pass (the three existing App locale tests + the five new CaptureToggle tests + the five existing TabStrip tests).

- [ ] **Step 4: Run type-check + build**

```powershell
npx tsc --noEmit
npm run build
```

Expected: both clean.

- [ ] **Step 5: Commit**

```powershell
git add src/App.tsx src/__tests__/i18n.test.tsx
git commit -m "feat(ui): mount CaptureToggle in footer with status polling"
```

---

## Task 16: Manual acceptance pass

**Files:** None.

This is the Phase 2 acceptance gate. The executor — a fresh subagent — cannot do GUI verification, so this task is for the human at the end of the plan. The subagent's job is only to (a) reset `%APPDATA%\voicetabs\audio\` so the test starts clean, and (b) report the directory state at the end via a follow-up after the human confirms.

- [ ] **Step 1: Clear the audio directory so the test starts from zero**

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs\audio" -ErrorAction SilentlyContinue
```

- [ ] **Step 2: Confirm `cargo test` is fully green**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: 37 passed.

- [ ] **Step 3: Confirm `npm test` is fully green**

```powershell
npm test
```

Expected: 13 passed.

- [ ] **Step 4: Confirm `npm run tauri build -- --debug` produces an installer**

```powershell
npm run tauri build -- --debug
```

Expected: `target\debug\bundle\nsis\VoiceTabs_*_x64-setup.exe` exists. **Note**: this build will take longer than Phase 1's because `voice_activity_detector` + `ort` add another ~30 MB of dependency compilation. Allow 15–25 minutes cold cache.

- [ ] **Step 5: Document the manual acceptance script for the human**

Write a one-page acceptance recipe to `docs/phase-2-acceptance.md`:

```markdown
# Phase 2 — manual acceptance

## Setup

Close any running VoiceTabs instance. Then:

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs\audio" -ErrorAction SilentlyContinue
npm run tauri dev
```

A window opens.

## Tests

1. **No capture by default**
   - Footer shows `🎤 Capturar: OFF` (or `🎤 Capture: OFF` in English).
   - `%APPDATA%\voicetabs\audio\` is empty (or non-existent).

2. **One utterance → one file**
   - Click the toggle. It changes to `🎤 Capturando` in green; OS shows the mic indicator.
   - Speak one short sentence (≈3 s), then pause ~1 s.
   - In `%APPDATA%\voicetabs\audio\` a new `<timestamp>.wav` appears.
   - Open the file in any media player; you should hear your sentence with a brief silence at the start (pre-roll).

3. **Two utterances → two files**
   - Continue capturing. Speak a second short sentence. Pause.
   - A second `<timestamp>.wav` file appears.

4. **60 s of silence → zero new files**
   - Stop speaking. Don't make noise (no typing nearby, please). Wait 60 s.
   - The file count in the audio directory is unchanged.

5. **Max-cap split**
   - Set a stopwatch. Speak continuously for at least 30 s (read aloud from any text).
   - Two files should appear: the first one capped at ~30 s, the second one starting where the first left off and ending when you stop.

6. **Toggle off**
   - Click the toggle. It changes back to `🎤 Capturar: OFF`. The OS mic indicator disappears.
   - Speaking should produce no new files until you toggle it back on.

7. **Restart**
   - Close the window. Re-run `npm run tauri dev`.
   - Footer toggle should be OFF again (capture state is not persisted across launches).
   - Files written in this session remain in the audio dir.

If any step fails, capture the symptom and stop. If everything passes, write a short stop-and-report covering: which device cpal selected (the toggle's title tooltip shows it), approximate utterance count from steps 2–5, and whether any errors appeared in the log (`%APPDATA%\voicetabs\logs\`).
```

- [ ] **Step 6: Commit the acceptance recipe**

```powershell
git add docs/phase-2-acceptance.md
git commit -m "docs: phase 2 manual acceptance recipe"
```

- [ ] **Step 7: Push to remote**

```powershell
git push origin master
```

CI should run and pass; if it fails because of, e.g., an `ort` binary download issue on the runner, the failure surfaces here, before the human runs the manual checks. Watch the run with `gh run watch` and report the outcome.

---

## End of Phase 2 — checkpoint

**Stop here and produce a stop-and-report.** Then we plan Phase 3 (STT subprocess + first-run wizard) in a separate session.

**What's verified at this checkpoint:**
- A1, A2, A3, A4 — not yet (no STT, no tabs binding, no wizard, no PTT).
- A5 (60 s silence → zero text) — **partial: 60 s silence → zero WAVs**. Becomes full A5 once STT lands and the segments table writes happen.
- A6, A7 — not applicable.
- A8 (persistence) — tabs + settings still persist; WAVs accumulate on disk across restarts.
- L1, L2 — not applicable (no STT yet).

**What's verified that isn't in the acceptance list but matters:**
- The VAD hysteresis state machine has 8 unit tests covering its core invariants (no flutter, brief bursts ignored, brief pauses tolerated).
- The utterance builder has 5 unit tests covering pre-roll backfill, falling-edge finalization, no-rising falling, max-cap, and two-utterances sequencing.
- The WAV writer has 3 unit tests for spec, clipping, and empty input.
- The capture controller compiles + the full Rust test suite stays green.
