# VoiceTabs — Design Document

- **Date:** 2026-05-20
- **Status:** Draft for review
- **Target:** Windows 11, v1
- **Primary language (UI):** Brazilian Portuguese (PT-BR), with English as a switchable option

---

## 1. What we are building

A tabbed text editor where notes are dictated, not typed. Each tab is an independent document. Whatever the user says into the microphone gets transcribed and appended to whichever tab was active **at the moment speech began** — even if the user switches tabs mid-sentence.

Two capture modes, switchable from the UI:

- **Always-on:** the app listens continuously and writes when it detects speech.
- **Push-to-talk (hold):** the app only captures while a user-bound global hotkey or mouse-button is held. Works while the app is unfocused or minimized to tray.

The full feature brief that produced this spec lives in chat history; relevant constraints are repeated below where they drive design.

## 2. Non-goals (v1)

Real-time / streaming transcription with token-by-token display, multiple-speaker diarization, cloud sync, accounts, sharing, audio-file imports, translation, mobile. **Single-user, single-machine, fully offline after first-run setup.**

> **Phase 7 amendment (2026-05-23):** the "fully offline" property holds for the
> default install. Phase 7 adds an **opt-in** cloud transcription backend
> (OpenAI `gpt-4o-mini-transcribe`) that the user can select from the settings
> drawer. When selected, per-utterance audio is uploaded to `api.openai.com`
> over HTTPS; the API key is stored in the OS credential store and never
> logged or persisted to SQLite. Local CPU transcription remains the default
> and is fully unaffected. See
> `docs/superpowers/plans/2026-05-23-voicetabs-phase-7-openai-backend.md`.

## 3. Quality bar & acceptance criteria

| ID | Criterion |
|---|---|
| A1 | Double-click installer → app works without any other action. |
| A2 | First launch with no model walks the user through download entirely in-app. |
| A3 | Three tabs; speaking into each routes correctly; mid-sentence tab switch keeps the sentence whole in the originally-active tab. |
| A4 | PTT works with the app minimized or unfocused. |
| A5 | 60 seconds of silence produces zero segments. |
| A6 | Custom vocabulary terms demonstrably improve transcription accuracy on those terms. |
| A7 | Killing `stt_worker.exe` externally: app detects and recovers within 5 s with no user action. |
| A8 | Restart the app: tabs, segments, audio, and settings are all preserved. |
| L1 | End of utterance → final text visible: **≤ 1.5 s** on the reference hardware (NVIDIA GTX 1060 6 GB). |
| L2 | No partial / streaming UI ("tokens piscando"). Each segment appears once, final. |

## 4. Tech stack (locked decisions)

| Layer | Choice |
|---|---|
| App shell | **Tauri 2.x** (Rust core + WebView2 frontend) |
| Frontend | **React 18 + TypeScript + Vite**; state via **Zustand**; i18n via **react-i18next**; custom segment-list view (not a rich-text editor) |
| STT engine | **whisper.cpp** via **`whisper-rs`**, hosted in a *separate* `stt_worker.exe` subprocess |
| Default model | Chosen by **first-launch benchmark**. Wizard recommends `large-v3-turbo-q5_0` (~590 MB) if a CUDA GPU is detected, else `small-q5_0` (~360 MB). If the recommended model fails the latency budget, the wizard falls back along the chain `large-v3-turbo-q5_0 → medium-q5_0 (~540 MB) → small-q5_0 → tiny-q5_0 (~78 MB)`. All four are available in the wizard's "Advanced" view. User can change later in Settings. |
| GPU backend | CUDA build for NVIDIA (Pascal cc 6.1+); CPU build as fallback. Both `stt_worker` builds shipped in the installer; the right one is launched at runtime based on detection. |
| VAD | **Silero VAD** (ONNX, ~2 MB) via the **`ort`** Rust crate |
| Audio capture | **`cpal`** at 16 kHz mono f32 with a 500 ms pre-roll ring buffer |
| Audio storage | **WAV PCM 16 kHz mono** at `%APPDATA%\voicetabs\audio\<segment_id>.wav` |
| Persistence | **SQLite** via **`rusqlite`** (bundled feature) at `%APPDATA%\voicetabs\voicetabs.db` |
| Keyboard hotkeys | **`tauri-plugin-global-shortcut`** (press + release events) |
| Mouse-button hotkeys | Custom Win32 low-level mouse hook (`SetWindowsHookExW(WH_MOUSE_LL, …)`) via the **`windows`** crate, on a dedicated thread |
| System tray | **`tauri-plugin-tray-icon`** |
| Logs | **`tracing`** + **`tracing-appender`** rolling logs at `%APPDATA%\voicetabs\logs\` |
| Packaging | **Tauri NSIS bundler** → one `.exe` installer (model excluded; downloaded on first launch) |
| HTTP (model download) | **`reqwest`** with Range/resume; SHA-256 checksum verify; atomic rename from `.partial` |

**Rejected alternatives:** Electron (would need `uIOhook-napi` for hold-detection on global keys — single-maintainer dependency — and adds ~70 MB Chromium/Node with no offsetting benefit). .NET WinUI (packaging fine, but segment-list UX is faster to build in React). faster-whisper (forces a Python runtime, fighting the "no Python" requirement). Vosk (lower accuracy on PT-BR than even small Whisper). Vulkan whisper.cpp backend (broader GPU support but slower than CUDA on the reference hardware; revisit if installer size or non-NVIDIA support becomes a goal).

## 5. Architecture

### 5.1 Processes

Three Win32 processes from one installer:

```
┌─────────────────────────────────────────────────────────────────┐
│  voicetabs.exe       (main process)                             │
│  ┌──────────┐  ┌──────┐  ┌──────────┐  ┌─────────┐  ┌────────┐  │
│  │ AudioIn  │→ │ VAD  │→ │ Utterance│→ │ STTClient│ │ DB     │  │
│  │ (cpal)   │  │      │  │ Builder  │  │ (IPC)   │  │ SQLite │  │
│  └──────────┘  └──────┘  └────┬─────┘  └────┬────┘  └───┬────┘  │
│                               │             │           │       │
│                               ▼             ▼           │       │
│                       ┌───────────────┐ ┌──────────┐    │       │
│                       │  TabRouter    │ │ Hallu-   │    │       │
│                       │ (start_tab_id │ │ cination │    │       │
│                       │  per utt.)    │ │  Filter  │    │       │
│                       └──────┬────────┘ └────┬─────┘    │       │
│                              │               └──────────┤       │
│                              └────────► SegmentStore ◄──┘       │
│                                                                 │
│  HotkeyManager  ──── keyboard via tauri-plugin-global-shortcut  │
│                  ┴── mouse via WH_MOUSE_LL hook (own thread)    │
│                                                                 │
│  Supervisor: monitors stt_worker child; restarts on exit        │
│  Tauri events ◄────────────────────────────────────────────┐    │
│                                                            │    │
│  ┌───────────────────────────────────────────────────────┐ │    │
│  │ WebView2 frontend (React)                             │ │    │
│  │ - Tabs / Segments view                                │ │    │
│  │ - Settings, First-Run Wizard, Tray menu               │◄┘    │
│  └───────────────────────────────────────────────────────┘      │
└──────────────────────────────────┬──────────────────────────────┘
                                   │ stdio (JSON line + length-prefixed PCM)
                                   ▼
                ┌─────────────────────────────────────┐
                │  stt_worker.exe   (whisper-rs)      │
                │  - loads model once at startup      │
                │  - per request: PCM in → text out   │
                │  - exits on fatal error             │
                └─────────────────────────────────────┘
```

### 5.2 One-utterance dataflow

1. `AudioInput` (cpal) streams 16 kHz mono f32 frames into a 500 ms ring buffer (continuous in always-on, gated by hotkey state in PTT).
2. `VAD` (Silero) runs on each 30 ms frame. State machine:
   - **Idle → Speaking** when probability ≥ 0.5 sustained for ≥ 120 ms.
   - **Speaking → Idle** when probability ≤ 0.35 sustained for ≥ 700 ms (configurable: `vad_silence_ms`).
   - In PTT mode, hotkey release forces Speaking → Idle immediately.
3. At the **rising edge** ("Idle → Speaking"), `TabRouter` snapshots `start_tab_id` from the current frontend selection (atomic read of an `AtomicU64` updated whenever the user clicks a tab). This snapshot is the only thing that matters for routing.
4. Pre-roll (500 ms before rising edge) + ongoing frames are appended to a per-utterance `Vec<f32>` buffer.
5. End conditions for the buffer:
   - Falling edge (always-on), or
   - Hotkey release (PTT), or
   - 30 s hard cap (configurable: `max_utterance_ms`) — emits the segment, restarts a new utterance if still speaking.
6. The buffer is handed to `STTClient`, which writes a JSON header `{request_id, sample_rate: 16000, language: "pt", initial_prompt: "<vocab snippet>", n_samples}` followed by the raw f32 PCM bytes to `stt_worker`'s stdin.
7. Worker responds with JSON: `{request_id, text, avg_logprob, no_speech_prob, duration_ms}`.
8. `Hallucination Filter` evaluates and drops if any:
   - **Blocklist match** (normalized lowercased substring): `"obrigado por assistir"`, `"legendas pela comunidade amara.org"`, `"thanks for watching"`, `"thank you for watching"`, `"subtitles by"`, `"subscribe to my channel"`, plus a small extensible list.
   - `no_speech_prob > 0.6`
   - `avg_logprob < -1.0`
   - Utterance audio RMS < −45 dBFS (belt-and-suspenders; VAD usually catches this first)
   - Empty / whitespace-only text after trim
9. If kept: `SegmentStore` inserts the row + writes the WAV file; emits `segment-created` Tauri event with `{tab_id: start_tab_id, segment}`.
10. Frontend appends the segment card to the bottom of `start_tab_id`'s list.

**Tab routing invariant:** `start_tab_id` is read once, atomically, at step 3. Nothing past that point cares about the current frontend selection. This is the entire mechanism that satisfies A3.

### 5.3 Supervisor / crash recovery

A Tokio task owns the `Child` handle for `stt_worker`. It:

- Spawns the worker at startup with the chosen model path on the command line.
- Awaits the model-ready handshake on the framed protocol (worker emits the `{"ready": true, ...}` frame defined in §7.4 once the model is loaded).
- On any of: child exit, broken pipe writing a request, no response within 10 s — logs the cause, kills any lingering process, respawns within 500 ms, re-handshakes.
- Maintains a small in-memory queue (max 3 pending utterances). If queue grows beyond that during a long recovery, oldest are dropped and the frontend gets a non-blocking toast.
- Emits `stt-status` events to the frontend (`ready` / `restarting` / `degraded`). The tab footer shows a yellow dot when not `ready`; no modal, no blocking UI.
- The model file is loaded via memory-mapped I/O so warm-cache reloads complete in ~1–2 s on the reference hardware. Cold cache may extend recovery to ~5–8 s the first time after boot; subsequent recoveries are within A7's 5 s budget.

If we measure cold-cache recovery consistently exceeding 5 s, the fallback is the **hot spare** pattern (always keep two workers, swap on death) — costs ~600 MB extra RAM but guarantees sub-second recovery. Decision deferred to phase 7 based on actual numbers.

## 6. Data model

### 6.1 SQLite schema

```sql
CREATE TABLE tabs (
  id            INTEGER PRIMARY KEY,
  title         TEXT NOT NULL,
  order_idx     INTEGER NOT NULL,
  created_at    INTEGER NOT NULL,  -- unix ms
  updated_at    INTEGER NOT NULL
);

CREATE TABLE segments (
  id                INTEGER PRIMARY KEY,
  tab_id            INTEGER NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
  position          INTEGER NOT NULL,             -- order within the tab
  text              TEXT NOT NULL,                -- current (may be user-edited)
  original_text     TEXT NOT NULL,                -- as first transcribed
  audio_path        TEXT NOT NULL,                -- relative to %APPDATA%\voicetabs\audio\
  started_at        INTEGER NOT NULL,             -- unix ms
  ended_at          INTEGER NOT NULL,
  duration_ms       INTEGER NOT NULL,
  vocab_snapshot    TEXT NOT NULL,                -- JSON array of terms at transcription time
  avg_logprob       REAL NOT NULL,
  no_speech_prob    REAL NOT NULL,
  model_id          TEXT NOT NULL
);
CREATE INDEX idx_segments_tab ON segments(tab_id, position);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL                              -- JSON-encoded
);
```

Settings keys: `capture_mode`, `hotkey_binding` (JSON: `{kind: "key" | "mouse", code: string}`), `vocab_terms` (JSON array), `model_id`, `language` (`"pt"`), `ui_locale` (`"pt-BR"` or `"en"`), `vad_threshold_start`, `vad_threshold_end`, `vad_silence_ms`, `max_utterance_ms`, `mic_device_id` (null = default), `theme` (`"light"`/`"dark"`/`"system"`), `tray_close_to_tray` (default true).

### 6.2 File layout

```
%APPDATA%\voicetabs\
  voicetabs.db
  audio\
    <segment_id>.wav
  models\
    ggml-large-v3-turbo-q5_0.bin
    ggml-medium-q5_0.bin            (only present if downloaded)
    ggml-small-q5_0.bin             (only present if downloaded)
  logs\
    voicetabs.log                   (rolling, 5 × 5 MB)
    stt_worker.log
  config.lock                       (single-instance lock file)
```

## 7. Subsystem designs

### 7.1 AudioInput (cpal)

- Enumerates input devices at startup and on `WM_DEVICECHANGE`. Settings shows a dropdown; default = system default.
- Opens at 16 kHz mono f32; if the device doesn't support that natively, runs a Rust resampler (`rubato`) on the captured stream.
- Pushes frames into a SPSC ring buffer (500 ms capacity) → consumed by the VAD/utterance pipeline.
- Failure modes: device disappears → pause, emit `audio-status: lost`, retry every 2 s, resume on recovery. Mic-access denied at the OS level → instructional UI pointing to Settings → Privacy → Microphone.

### 7.2 VAD (Silero via ort)

- Loads `silero_vad.onnx` bundled in installer resources.
- State machine as in §5.2. Hysteresis prevents flutter near the threshold.
- Emits `VadEvent::RisingEdge { timestamp_ms }` and `VadEvent::FallingEdge { timestamp_ms }`.
- In PTT mode VAD still runs (as the *only* hallucination gate), but boundaries are derived from hotkey state, not VAD edges.

### 7.3 Utterance Builder + TabRouter

- Listens to VAD events and hotkey events.
- On utterance start: snapshots `start_tab_id` (single atomic load), allocates a buffer with pre-roll bytes copied from the ring buffer.
- On utterance end: hands `(buffer, start_tab_id, started_at, ended_at, vocab_snapshot)` to the STT client.

### 7.4 STT subprocess protocol

`stt_worker.exe` is a self-contained Rust binary. Two builds (`stt_worker_cuda.exe`, `stt_worker_cpu.exe`) are shipped; the main process picks based on GPU detection.

**Startup args:**
```
stt_worker --model <path> --language pt --threads <N>
```

**Protocol over stdio** (length-prefixed framing):
- All messages framed as: `<u32 LE length><bytes>`.
- `bytes` is either UTF-8 JSON (request/response headers) or raw audio.
- On startup, after the model loads, worker sends one JSON frame: `{"ready": true, "model_id": "...", "backend": "cuda"|"cpu"}`. Until then no requests are accepted.
- **Request:** JSON header `{"request_id": "<uuid>", "sample_rate": 16000, "language": "pt", "initial_prompt": "<vocab>", "n_samples": <u32>}` followed by exactly one `n_samples * 4` byte frame of `f32` PCM.
- **Response:** one JSON frame `{"request_id": "...", "text": "...", "avg_logprob": -0.3, "no_speech_prob": 0.02, "duration_ms": 450}` (or `{"request_id": "...", "error": "..."}`).
- Worker exits with status 0 on EOF on stdin, non-zero on any fatal error.

### 7.5 Hallucination Filter

Pure function: `(text, avg_logprob, no_speech_prob, rms_dbfs, blocklist) -> Decision::{Keep, Drop(reason)}`. Easy to unit-test. Reasons are logged for diagnostics.

### 7.6 HotkeyManager

Two subsystems behind one trait:

```rust
trait HotkeyBackend {
    fn capture_next_press(&self) -> Result<Binding>;     // blocking, Esc cancels
    fn watch(&self, binding: Binding, on_press: Box<dyn Fn() + Send>, on_release: Box<dyn Fn() + Send>);
    fn unwatch(&self);
}
```

- **KeyboardBackend** wraps `tauri-plugin-global-shortcut`. The plugin's v2 `ShortcutState::Pressed` and `ShortcutState::Released` map to `on_press` / `on_release`.
- **MouseBackend** owns a dedicated OS thread that installs `WH_MOUSE_LL`, pumps the message loop, and forwards `WM_XBUTTONDOWN` / `WM_XBUTTONUP` (and any other bound mouse button) to channels. We never modify the event — the hook is read-only.
- A `HotkeyManager` composes both and dispatches based on the active binding's `kind`.
- **Capture UI:** user clicks "Bind hotkey" in settings → manager arms both backends in capture mode → the next physical keyboard key or mouse button press is captured, displayed (`"Right Ctrl"` / `"Mouse Button 5"`) and saved. Esc cancels.

### 7.7 Persistence

- One `rusqlite::Connection` per main process, behind a `Mutex<Connection>` (write rate is low — a few writes/sec at most).
- WAL mode; `synchronous=NORMAL`; foreign keys on.
- Migrations as ordered SQL files in `src-tauri/migrations/`, applied at startup.
- Audio files are written **before** the segment row is inserted; if the insert fails, the orphan file is reaped at next startup.

### 7.8 System tray

Tray menu: `Mostrar VoiceTabs` / `Show VoiceTabs`, `Modo: Sempre ativo / Push-to-talk` (toggle), `Sair / Quit`. Left-click toggles window visibility. Tray icon shows a small dot to indicate capture state (green = ready, red = capturing, yellow = recovering).

### 7.9 First-run wizard

A multi-step UI inside the app, rendered before the main view if `settings.first_run_completed` is false. Steps:

1. **Welcome** — language (PT-BR / English; pre-selected from Windows UI locale).
2. **Microphone check** — VU meter; user speaks; we confirm signal. If no signal → instructional fallback.
3. **GPU detection** — call `nvidia-smi` if present and parse `--query-gpu=name,memory.total,compute_cap`. Fallback to a `cuda` runtime probe via the `stt_worker_cuda.exe` `--probe` flag (returns JSON; non-zero exit = no CUDA). Show outcome plainly: "Detected: NVIDIA GTX 1060 (6 GB, CUDA)" or "No GPU detected — using CPU".
4. **Model download** — recommend `large-v3-turbo-q5_0` if GPU else `small-q5_0`; "Advanced" view exposes the full chain from §4. Show size, expected accuracy/speed. Progress bar with resume. Verify SHA-256.
5. **Benchmark** — once the model is loaded, transcribe the bundled 5-second test clip; measure end-to-end. If > 1.3 s, the wizard automatically offers the next smaller model in the chain (§4) and re-benchmarks. Shows the final result: "End-to-end latency: 720 ms — ready."
6. **Bind PTT hotkey (optional)** — "Press the key or mouse button you'd like to use, or skip to use Always-on."
7. **Done** → main UI.

### 7.10 i18n

- `react-i18next` with two locale bundles: `pt-BR.json` (default) and `en.json`.
- Locale auto-selected on first launch from `navigator.language` (web view exposes Windows UI locale); switchable from Settings → Language.
- All Rust-side errors that surface in the UI carry a stable error code; the frontend maps codes → localized messages. No raw Rust strings shown to the user.

### 7.11 Capture modes

- `capture_mode` setting toggles between `"always_on"` and `"ptt"`.
- The UI shows the current mode in the tab footer and tray icon menu.
- Switching modes does not interrupt an in-flight utterance.
- In `"ptt"` mode with no hotkey bound, capture is disabled and the UI nudges the user to bind one.

### 7.12 Segment UI

Each tab is a vertical list of segment cards. A card is **not** a textarea — it's a paragraph with:

- The transcribed text.
- A subtle play button on hover → plays the original `.wav` via an HTML `<audio>` element.
- An edit button → swaps the paragraph for a textarea bound to the segment's `text`. Save / Cancel.
- An overflow menu: "Re-transcribe (current vocab)", "Re-transcribe (snapshot vocab)", "Delete".

The tab body is the ordered concatenation of segments. Selecting text across segment boundaries to copy works because the DOM concatenates them; there is no global blob in the data model.

## 8. UI overview (text walkthrough)

Top bar: tab strip (drag to reorder, double-click to rename, "×" to close with confirm if non-empty, "+" to create). Each tab title editable inline.

Main area: scrolling list of segment cards. Newest at the bottom; auto-scrolls when a new segment lands in the active tab. A subtle pulse on the receiving tab when a segment lands in a *different* tab than the one currently shown.

Tab footer (always visible, ~28 px):
- Left: current capture mode (`Sempre ativo` / `Push-to-talk: Right Ctrl`).
- Center: status indicator (idle / capturing / recovering / no-mic / no-model).
- Right: settings gear → opens settings panel as a modal/drawer.

Settings drawer sections: Captura (modo, hotkey, microphone), Modelo (size, benchmark), Vocabulário (textarea, one term per line), Idioma, Tema, Sobre.

System tray: see §7.8.

## 9. Packaging & install layout

**Installer contents (one NSIS `.exe`, ~280 MB):**
```
voicetabs.exe
stt_worker_cuda.exe
stt_worker_cpu.exe
WebView2Loader.dll                  (Tauri runtime)
onnxruntime.dll                     (Silero VAD)
silero_vad.onnx
benchmark_clip_pt.wav               (5 s bundled test clip)
locales\
  pt-BR.json
  en.json
cuda\
  cudart64_12.dll
  cublas64_12.dll
  cublasLt64_12.dll
LICENSE.txt
```

**Not bundled (downloaded at first launch):** Whisper model `.bin` files.

**Install destination:** `%ProgramFiles%\VoiceTabs\` (default). User data at `%APPDATA%\voicetabs\` (per user).

**Code signing:** out of scope for v1; SmartScreen warning expected. Add signing in a later release.

**Uninstall:** removes `%ProgramFiles%\VoiceTabs\`. Offers to also remove `%APPDATA%\voicetabs\` (which holds models, db, audio).

**Auto-update:** **not in v1.** User re-runs the installer to update; in-place upgrade preserves user data.

## 10. Phasing (each ends with a stop-and-report)

| Phase | Scope | Acceptance |
|---|---|---|
| 0 | `git init`, repo skeleton, `tauri init`, CI smoke build | Build produces a `voicetabs.exe` that opens a window saying "VoiceTabs" |
| 1 | Tab UI + SQLite schema + settings persistence | Create/rename/reorder/close tabs; survive restart (A8 partial) |
| 2 | Audio capture (cpal) + VAD (Silero) + utterance buffering (no STT yet) | Speaking produces utterance WAVs on disk; 60 s silence → 0 WAVs (A5 partial) |
| 3 | `stt_worker` + first-run wizard (GPU detect + model download + benchmark) | Hardcoded WAV → text round-trip; wizard works from a clean `%APPDATA%` (A2) |
| 4 | Wire utterance → STT → hallucination filter → segments; tab routing | A3, A5 (full), A6, L1, L2 |
| 5 | Capture modes + HotkeyManager (keyboard + mouse) + tray | A4 |
| 6 | Segment UI (play, edit, delete, re-transcribe) + vocabulary settings | A6 (full) |
| 7 | Supervisor / crash recovery polish | A7 |
| 8 | NSIS installer + fresh-Windows VM acceptance test | A1, A8 (full) |

Each phase ends with a **stop-and-report**: a short written summary of what works, what doesn't, what was changed from this spec, and what to test before approving the next phase.

## 11. Testing strategy

- **Unit tests (Rust):** VAD state machine, utterance builder boundaries, tab routing invariant, hallucination filter (with table-driven fixtures), STT IPC framing, settings persistence migrations.
- **Unit tests (TS):** segment list rendering, tab strip interactions, settings form, i18n string presence.
- **Integration tests:** `stt_worker` round-trip with a tiny test model and a canned WAV; supervisor restart drill (kill child, assert recovery + queue replay).
- **Acceptance suite:** an automated end-to-end harness that exercises each of A1–A8 against a real build on a fresh Windows 11 VM. Manual gate on L1/L2 (latency and "no flicker") because they're subjective UI claims.

## 12. Risks & decisions deferred

| Risk | Mitigation / Decision |
|---|---|
| CUDA recovery exceeds 5 s cold cache (A7) | Phase 7: measure; if needed, switch to hot-spare worker pattern (+~600 MB RAM). |
| SmartScreen warnings on unsigned installer | Document; add code signing in a later release. |
| Antivirus flagging the WH_MOUSE_LL hook | Hook is minimal and read-only; we document its purpose. If false positives become real, gate the mouse backend behind a setting and default to keyboard-only. |
| Mouse-button binding requires the app to be running for PTT (obvious but worth stating) | Standard "always-running with tray" pattern handles this. |
| Model download host availability | Use multiple mirrors; allow user to paste a custom URL in advanced settings. |
| Long monologues exceeding the 30 s cap split into multiple segments mid-sentence | Acceptable — the cap is rare in practice; configurable. The brief's tab-routing invariant applies per utterance, so a forced split would land both pieces in `start_tab_id` anyway. |
| Mic device changes mid-utterance | We pause the in-flight utterance, drop it, and resume on the new default device. Documented behavior. |

## 13. Open questions

None blocking the start of phase 0. The following are decisions we'll revisit with data once the relevant phase lands:

- Cold-cache STT recovery latency on the reference hardware (phase 7 — measure before committing to the hot-spare pattern).
- Final segment-card affordances beyond play / edit / delete / re-transcribe (e.g., timestamp display, copy-this-segment) — deferred to phase 6 once the basic surface is in the user's hands.
