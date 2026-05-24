# VoiceTabs — Phase 7 (Hybrid Local-CPU / OpenAI Backend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the speech-to-text backend selectable at runtime from the settings drawer. Today the only backend is the local `stt_worker` subprocess (CPU whisper.cpp). Phase 7 adds a second backend that calls OpenAI's `gpt-4o-mini-transcribe` HTTP endpoint per utterance. Both backends sit behind a single `SttBackend` trait; the drainer and `segments_retranscribe` call the trait, not the supervisor directly. A user can flip from "Local (CPU)" to "OpenAI" mid-session and the very next utterance is routed through the cloud.

**Architecture:** Introduce `trait SttBackend: Send + Sync` in `src-tauri/src/stt/backend.rs`. Two implementations: `LocalSttBackend` wraps the existing `SttSupervisor` (no behavioural change for the local path); `OpenAiSttBackend` owns a `reqwest::Client`, an API key fetched from the OS keyring on construction, and a small retry helper. A new `ActiveBackend` handle (`Arc<TokioRwLock<Arc<dyn SttBackend>>>`) is stored in Tauri's `manage()` slot; the supervisor itself stays managed too so the local backend can keep its current crash-recovery semantics. A `set_backend(kind, app_handle)` helper rebuilds the inner `Arc<dyn SttBackend>` and updates `SttStatusHandle` so the frontend status dot flips immediately. API keys live in the OS credential store via the `keyring` crate (Windows backs onto Credential Manager / DPAPI); the SQLite `settings` table holds only a boolean indicating "a key is configured".

**Spec divergence — DOCUMENT UP FRONT.** The original design (`docs/superpowers/specs/2026-05-20-voicetabs-design.md` §2) said: _"Single-user, single-machine, fully offline after first-run setup."_ Phase 7 adds an **opt-in** cloud transcription path. Offline still works (it remains the default for any user who never touches the setting). When the cloud backend is selected, the settings drawer shows a privacy disclaimer that audio is uploaded to `api.openai.com`. No telemetry is added; no key material ever leaves the local Credential Manager except as a `Bearer` token in the per-utterance HTTPS request. The design doc itself is updated in Task 1 to record this divergence so the spec and the code stay in sync.

**Tech Stack additions:**
- `keyring = "3"` — cross-platform secret store. Windows uses the Win32 Credential Manager (which itself uses DPAPI). Verified that the 3.x line targets Win11 cleanly; no extra system deps.
- `reqwest = { version = "0.12", default-features = false, features = ["json", "multipart", "rustls-tls"] }` — HTTP client. We pick `rustls-tls` to avoid linking against the system OpenSSL.
- `tokio` already present; no new features required (we use `time::sleep` and `sync::RwLock` which are already enabled).
- `mockito = "1"` (dev-dependency) — local HTTP server for unit-testing the OpenAI backend without hitting the network.

**Reference spec:** `docs/superpowers/specs/2026-05-20-voicetabs-design.md`. The only directly-relevant section is §2 (non-goals — explicitly amended by this plan); §5.2 (one-utterance dataflow — unchanged conceptually, only the STT step is now polymorphic); §7.5 (hallucination filter — runs on cloud output too).

**Builds on:** Phase 3 (`docs/superpowers/plans/2026-05-22-voicetabs-phase-3-stt-subprocess.md`), Phase 4 (`docs/superpowers/plans/2026-05-22-voicetabs-phase-4-segments.md`), CUDA-removal commit `2ffb195`. Starting state: HEAD `2ffb195` on `master`, CI green, **73+ Rust lib tests + 33 TS tests passing**. Phase 5 (hotkey + tray) and Phase 6 (segment polish) are drafted in parallel and merge ahead of this plan; they do not touch the STT layer.

---

## Architecture decisions LOCKED for this phase

These are firm. The tasks below implement them as-is.

1. **One trait, two implementations.** `trait SttBackend: Send + Sync { async fn transcribe(&self, req: TranscribeRequest) -> Result<TranscriptionResult, BackendError>; fn backend_id(&self) -> &str; fn model_id(&self) -> &str; }`. `TranscribeRequest` carries `{request_id, samples: Vec<f32>, sample_rate: u32, language, initial_prompt, audio_path: Option<PathBuf>, started_at_ms}`. Both backends return the existing `TranscriptionResult` type from `stt::protocol`. The OpenAI backend fakes `avg_logprob = 0.0`, `no_speech_prob = 0.0` since the API does not expose those numbers.
2. **The supervisor stays.** `LocalSttBackend` is a thin wrapper around `SttSupervisor`. The supervisor keeps its crash-recovery + replay logic; the trait wrapper just adapts the call signature. We do **not** flatten the supervisor into the trait — that would lose the in-flight replay guarantee.
3. **Active backend is hot-swappable.** Stored as `ActiveBackend(Arc<TokioRwLock<Arc<dyn SttBackend>>>)`. Hot-path callers (`run_segment_pipeline`, `segments_retranscribe`) snapshot the inner `Arc<dyn SttBackend>` with a short read-lock, drop the lock, then call `.transcribe(...)`. Backend swap takes the write lock for ~microseconds and replaces the inner Arc. In-flight requests started before the swap finish on the **old** backend's `Arc` they already cloned — no aborts, no orphan results. Worst case: switching from Local → OpenAI mid-utterance means that one utterance lands locally and every subsequent one goes to OpenAI; this is correct behaviour, not a bug.
4. **OpenAI call shape, non-streaming.** `POST https://api.openai.com/v1/audio/transcriptions`. Multipart form with `model=gpt-4o-mini-transcribe`, `file=<wav bytes>` (built in memory with `hound::WavWriter` against a `Cursor<Vec<u8>>` — no tempfile), `language=pt` (or whatever the request carries), `prompt=<initial_prompt>`, `response_format=json`. Header: `Authorization: Bearer <key>`. Streaming (`stream: true` SSE) is a **stretch goal** noted at the end of the plan, not implemented in v1.
5. **API key storage = OS keyring.** Service `voicetabs`, key `openai_api_key`. The `settings` table stores only `openai_api_key_set: bool` for UI display. The key value is never written to SQLite, never logged, never sent over Tauri events. The OpenAI backend reads the key from the keyring at construction time; if absent, construction returns `BackendError::MissingApiKey` and the caller (settings change handler) falls back to the local backend and surfaces an error toast.
6. **Backend persistence.** A new settings key `stt_backend` stores `"local"` or `"openai"` as JSON. On app boot we read it; default `"local"` if missing. The active backend handle is initialised from this value before the Tauri runtime starts handling requests.
7. **Error handling for OpenAI.**
   - Network errors (`reqwest::Error::is_connect`, `is_timeout`, body read failures) → `BackendError::Network(scrubbed)`. The drainer logs and drops the utterance; the WAV stays on disk.
   - `401` / `403` → `BackendError::Auth`. Sets `SttStatus::Error { message: "OpenAI: chave inválida" }` (PT-BR; the message is structured so the frontend can localize). The status dot turns red. Subsequent utterances keep failing fast until the user updates the key.
   - `429` → retry once after `exponential_backoff(attempt=1, base_ms=500, jitter=true)` (max ~750 ms). Second `429` → `BackendError::RateLimited`, log, drop utterance.
   - `5xx` → log + fail this utterance, no retry.
   - **Every error path scrubs the API key from any string we log.** The wrapper helpers in `openai.rs` route every error through `scrub(err.to_string(), &api_key)` before returning.
8. **Tracing redaction is belt-and-suspenders.** A small `redact_secret` helper masks the API key anywhere it appears in a `String`. The OpenAI backend struct holds the key as `secrecy::SecretString` so accidental `Debug` prints render as `[REDACTED]`. (`secrecy` is already a transitive dep of `keyring`; if not, add it as a direct dep — Task 3 confirms during cargo check.)
9. **Frontend status dot reflects backend identifier.** The existing `prettyBackend()` in `SttStatusDot.tsx` already maps `"openai" → "OpenAI"` (verified — see file). When the user switches backend the supervisor publishes a fresh `SttStatus::Ready { backend: "openai", model_id: "gpt-4o-mini-transcribe" }` (for OpenAI) or `{ backend: "cpu", model_id: <whisper-model> }` (for Local) and the frontend reflects it within one polling interval.
10. **The hardcoded `"cpu"` literal in `lib.rs::run()` goes away.** Replaced by a value derived from the persisted `stt_backend` setting at boot. For the local path this still resolves to `"cpu"`; for the OpenAI path it is `"openai"`. The startup banner string is updated to read from the active backend instead of hard-coding "CPU (local)".
11. **CUDA verification is Task 0.** Quick grep + fix: rip the two stale comments referencing CUDA in `src-tauri/src/lib.rs` and `src-tauri/tests/stt_supervisor_test.rs`. Confirm zero `cuda`/`gpu`/`nvidia` matches in `src-tauri/src` and `stt_worker`; confirm zero `stt_worker_cuda` matches across the repo (docs aside).

---

## Acceptance for this plan

- **A-P7-1.** With `stt_backend=local` (default) and no API key configured, the app behaves exactly as today: capture → speak → segment card appears. No regressions on existing Phase 3/4 manual acceptance.
- **A-P7-2.** From the settings drawer the user can paste an API key, click Save, see "Chave configurada ✓". The key is written to Windows Credential Manager (verifiable via `cmdkey /list`); it does **not** appear in `voicetabs.db` (verify with `sqlite3 voicetabs.db "select * from settings;"`) and does **not** appear in `voicetabs.log`.
- **A-P7-3.** Select "OpenAI" radio button → the active backend swaps. Status dot title flips from `(Local CPU · ggml-small-q5_1)` to `(OpenAI · gpt-4o-mini-transcribe)`. Speak a PT-BR sentence → segment card appears under the active tab with the transcribed text. WAV file is still saved locally.
- **A-P7-4 (mid-session swap).** Speak one utterance with `backend=local`, see segment. Open drawer, switch to `openai`, close drawer. Speak another utterance, see segment. Both cards appear in the same tab, in order. The first card's `model_id` column is the local whisper model; the second's is `gpt-4o-mini-transcribe`.
- **A-P7-5.** Set an invalid API key (e.g. `sk-invalid`). Switch to OpenAI. Speak → no segment appears, the status dot is red, the title shows "OpenAI: chave inválida". Logs at `info` level show the failure with the key masked as `Bearer sk-i**********`.
- **A-P7-6.** Clear the API key from the drawer → keyring entry is deleted (verifiable via `cmdkey /list`), `openai_api_key_set` flips to `false`, the radio resets to "Local (CPU)" (we treat "OpenAI selected but no key" as auto-revert; see Task 9), status dot returns to green Local.
- **A-P7-7.** All 73+ Rust lib tests still pass. All 33 TS tests still pass. **+ ~10 new Rust unit tests** (trait abstraction, OpenAI multipart builder against `mockito`, key scrubber, backend selector). **+ ~3 new TS tests** (BackendSettings UI render + save + clear flows).
- **A-P7-8 (CI).** `npm run typecheck`, `npm test`, `cargo test --workspace` all green. No new clippy warnings (`cargo clippy --workspace --all-targets -- -D warnings`).
- **L1 (≤ 1.5 s end-to-end latency).** Inherited from prior phases. The OpenAI path is allowed to exceed 1.5 s — `gpt-4o-mini-transcribe` typically returns in 800–1500 ms for ~5 s clips, but network conditions vary. The plan does **not** add a latency assertion for the OpenAI backend.

## Out of scope (deferred)

- Streaming transcription (SSE `stream: true` against `gpt-4o-mini-transcribe`). Noted as stretch in Task 16; not delivered.
- Per-utterance cost estimation / monthly usage display.
- Multiple OpenAI organisations / keys.
- A "fallback to local if OpenAI fails" mode. We surface the error and stop; the user decides whether to switch back.
- Other cloud providers (Deepgram, AssemblyAI). Trait makes adding a third easy but no third backend ships in this plan.
- Re-transcribing existing segments through the cloud backend with vocab swap. The existing `segments_retranscribe` command automatically uses whichever backend is active when the user clicks "Re-transcrever" — that already works through the trait, no extra UI surfaces in this phase.

---

## File structure after this plan

```
src-tauri/
├── Cargo.toml                                # MODIFIED: + keyring, reqwest, secrecy, mockito (dev)
├── src/
│   ├── stt/
│   │   ├── mod.rs                            # MODIFIED: pub mod backend; pub mod openai; pub mod active
│   │   ├── backend.rs                        # NEW: trait SttBackend + TranscribeRequest + BackendError (TDD)
│   │   ├── local.rs                          # NEW: LocalSttBackend wrapping SttSupervisor
│   │   ├── openai.rs                         # NEW: OpenAiSttBackend (TDD against mockito)
│   │   ├── keyring.rs                        # NEW: get/set/clear API key with redact helpers (TDD)
│   │   ├── active.rs                         # NEW: ActiveBackend handle + set_backend swap helper
│   │   ├── supervisor.rs                     # unchanged (still owns local subprocess)
│   │   └── status.rs                         # unchanged
│   ├── commands/
│   │   ├── mod.rs                            # MODIFIED: pub mod backend
│   │   ├── backend.rs                        # NEW: backend_get / backend_set / openai_key_set / openai_key_clear
│   │   └── segments.rs                       # MODIFIED: retranscribe uses ActiveBackend instead of SttSupervisor directly
│   └── lib.rs                                # MODIFIED: read stt_backend setting, construct ActiveBackend, register new commands, drop hardcoded "cpu"
└── tests/
    ├── stt_backend_trait_test.rs             # NEW: stub backend round-trip
    └── openai_backend_test.rs                # NEW: mockito server, 200 / 401 / 429 / 5xx paths

src/
├── lib/tauri.ts                              # MODIFIED: + backendApi (get/set, openaiKey set/clear/status)
├── stores/settingsStore.ts                   # MODIFIED: + backend + openaiKeySet + setBackend + setOpenAiKey + clearOpenAiKey
├── components/
│   ├── SettingsDrawer.tsx                    # MODIFIED: render <BackendSettings/>
│   ├── BackendSettings.tsx                   # NEW: radio + key input + status + clear + disclaimer
│   └── SttStatusDot.tsx                      # unchanged (prettyBackend already covers "openai")
├── i18n/locales/{pt-BR,en}.json              # MODIFIED: + settings.backend.* + stt.errors.*
└── __tests__/
    ├── BackendSettings.test.tsx              # NEW
    └── settingsStore.test.ts                 # MODIFIED: cover backend + key set/clear flows
```

Each piece has one job:
- `stt::backend` is the trait + request/error types. Nothing else in there.
- `stt::local` is the trait impl over `SttSupervisor`. Pure adapter.
- `stt::openai` is the HTTP client + multipart builder + retry loop + scrubbing. Self-contained, mockable.
- `stt::keyring` wraps the `keyring` crate so the rest of the code never touches the crate directly; lets us swap to a different secret store if we ever add macOS / Linux.
- `stt::active` is the hot-swap handle.
- `commands::backend` is the thin Tauri-command layer. No business logic.

---

# Phase 7 tasks

## Task 0: CUDA scrub (pre-flight)

The repo is post-CUDA-removal (commit `2ffb195`). Two stale comments still reference CUDA — fix them so future readers don't think CUDA is still on the roadmap of this codebase.

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tests/stt_supervisor_test.rs`

- [ ] **Step 1: Verify the scope of stale references**

```powershell
# Should print exactly the two known stale references and nothing else.
Get-ChildItem -Recurse -Path src-tauri\src,src-tauri\tests,stt_worker\src `
  | Select-String -Pattern 'cuda|gpu|nvidia|stt_worker_cuda' -CaseSensitive:$false
```

If any match outside those two files turns up, STOP and surface it before continuing.

- [ ] **Step 2: Remove the CUDA comment in `src-tauri/src/lib.rs::resolve_model_path`**

Locate the block starting `// For now we bundle the \`small\` quantized model …`. Replace it with a comment that does not mention CUDA:

```rust
// We bundle the `small` quantized model (~330 MB) for usable CPU
// transcription. Cloud transcription via the OpenAI backend (Phase 7)
// bypasses this entirely and uses `gpt-4o-mini-transcribe`.
```

- [ ] **Step 3: Update the comment in `src-tauri/tests/stt_supervisor_test.rs`**

The line `// Backend was an enum from the removed stt::gpu module; backend is now a String.` stays accurate but is now ambient context. Leave it as-is unless the test file gains a `cuda`-suggestive new comment.

- [ ] **Step 4: Re-run the search**

```powershell
Get-ChildItem -Recurse -Path src-tauri\src,src-tauri\tests,stt_worker\src `
  | Select-String -Pattern 'cuda|gpu|nvidia|stt_worker_cuda' -CaseSensitive:$false
```

Expect: only the historical-comment line in `stt_supervisor_test.rs` (which is fine — it explains the post-removal state).

- [ ] **Step 5: `cargo check --workspace` is still green.**

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/lib.rs
git commit -m "chore(stt): drop stale CUDA-comment leftovers"
```

---

## Task 1: Spec divergence header in the design doc

Record the offline-only divergence in one place so spec readers find it immediately.

**Files:**
- Modify: `docs/superpowers/specs/2026-05-20-voicetabs-design.md`

- [ ] **Step 1: Append a "Phase 7 amendment" admonition under §2**

Right after the §2 "Non-goals" paragraph that ends with "**Single-user, single-machine, fully offline after first-run setup.**", insert:

```markdown
> **Phase 7 amendment (2026-05-23):** the "fully offline" property holds for the
> default install. Phase 7 adds an **opt-in** cloud transcription backend
> (OpenAI `gpt-4o-mini-transcribe`) that the user can select from the settings
> drawer. When selected, per-utterance audio is uploaded to `api.openai.com`
> over HTTPS; the API key is stored in the OS credential store and never
> logged or persisted to SQLite. Local CPU transcription remains the default
> and is fully unaffected. See
> `docs/superpowers/plans/2026-05-23-voicetabs-phase-7-openai-backend.md`.
```

- [ ] **Step 2: Commit**

```powershell
git add docs/superpowers/specs/2026-05-20-voicetabs-design.md
git commit -m "docs(spec): record Phase 7 opt-in cloud divergence"
```

---

## Task 2: Add Phase 7 dependencies

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the runtime + dev dependencies**

Append to the `[dependencies]` section of `src-tauri/Cargo.toml`:

```toml
keyring = "3"
reqwest = { version = "0.12", default-features = false, features = ["json", "multipart", "rustls-tls"] }
secrecy = "0.10"
```

Append to `[dev-dependencies]`:

```toml
mockito = "1"
```

- [ ] **Step 2: `cargo check --workspace` is green**

```powershell
cargo check --workspace
```

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/Cargo.toml Cargo.lock
git commit -m "chore(deps): keyring + reqwest + secrecy + mockito for Phase 7"
```

---

## Task 3: `SttBackend` trait + request/error types (TDD)

The trait and its data types. No implementations yet; just the contract + a stub for tests.

**Files:**
- Create: `src-tauri/src/stt/backend.rs`
- Modify: `src-tauri/src/stt/mod.rs`
- Create: `src-tauri/tests/stt_backend_trait_test.rs`

- [ ] **Step 1: Write the failing test first**

`src-tauri/tests/stt_backend_trait_test.rs`:

```rust
//! Sanity check that the trait is object-safe (we use `dyn SttBackend`) and that
//! a stub implementation routes one request to one response.

use std::sync::Arc;
use voicetabs_lib::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use voicetabs_lib::stt::protocol::TranscriptionResult;

struct StubBackend;

#[async_trait::async_trait]
impl SttBackend for StubBackend {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError> {
        Ok(TranscriptionResult {
            request_id: req.request_id,
            text: format!("stub-{}", req.samples.len()),
            avg_logprob: 0.0,
            no_speech_prob: 0.0,
            duration_ms: 10,
        })
    }
    fn backend_id(&self) -> &str { "stub" }
    fn model_id(&self) -> &str { "stub-model" }
}

#[tokio::test]
async fn trait_is_object_safe_and_routes_one_call() {
    let b: Arc<dyn SttBackend> = Arc::new(StubBackend);
    let req = TranscribeRequest {
        request_id: "r1".into(),
        samples: vec![0.0_f32; 16_000],
        sample_rate: 16_000,
        language: "pt".into(),
        initial_prompt: String::new(),
        audio_path: None,
        started_at_ms: 0,
    };
    let r = b.transcribe(req).await.unwrap();
    assert_eq!(r.text, "stub-16000");
    assert_eq!(b.backend_id(), "stub");
    assert_eq!(b.model_id(), "stub-model");
}
```

Run: `cargo test -p voicetabs --test stt_backend_trait_test` — must fail to compile because `stt::backend` doesn't exist yet.

- [ ] **Step 2: Add `async-trait` to `src-tauri/Cargo.toml`** (small, ubiquitous; trait wants `async fn` on `dyn`).

```toml
async-trait = "0.1"
```

- [ ] **Step 3: Implement the trait module**

`src-tauri/src/stt/backend.rs`:

```rust
//! `SttBackend` — abstract trait spanning local subprocess and cloud HTTP impls.
//!
//! Hot-path callers (`run_segment_pipeline`, `segments_retranscribe`) snapshot
//! an `Arc<dyn SttBackend>` from `ActiveBackend`, drop the lock, then call
//! `transcribe()`. See `stt::active::ActiveBackend` for the swap semantics.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;

use crate::stt::protocol::TranscriptionResult;

#[derive(Debug, Clone)]
pub struct TranscribeRequest {
    pub request_id: String,
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub language: String,
    pub initial_prompt: String,
    pub audio_path: Option<PathBuf>,
    pub started_at_ms: u64,
}

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendError {
    #[error("missing OpenAI API key")]
    MissingApiKey,
    #[error("authentication failed (check API key)")]
    Auth,
    #[error("network: {0}")]
    Network(String),
    #[error("rate limited")]
    RateLimited,
    #[error("server error: {0}")]
    Server(String),
    #[error("worker error: {0}")]
    Worker(String),
    #[error("backend not ready")]
    NotReady,
}

#[async_trait]
pub trait SttBackend: Send + Sync {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError>;
    fn backend_id(&self) -> &str;
    fn model_id(&self) -> &str;
}
```

- [ ] **Step 4: Re-export from `stt/mod.rs`**

```rust
pub mod backend;
// existing modules…
pub use backend::{BackendError, SttBackend, TranscribeRequest};
```

- [ ] **Step 5: Run the trait test — green.**

```powershell
cargo test -p voicetabs --test stt_backend_trait_test
```

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/stt/backend.rs src-tauri/src/stt/mod.rs src-tauri/tests/stt_backend_trait_test.rs src-tauri/Cargo.toml Cargo.lock
git commit -m "feat(stt): SttBackend trait + TranscribeRequest + BackendError"
```

---

## Task 4: `LocalSttBackend` adapter

Wrap the existing supervisor. This is mechanical — the goal is that swapping `sup.transcribe(...)` for `backend.transcribe(req)` in `lib.rs` is a no-op change.

**Files:**
- Create: `src-tauri/src/stt/local.rs`
- Modify: `src-tauri/src/stt/mod.rs`

- [ ] **Step 1: Create `local.rs`**

```rust
//! Adapter implementing `SttBackend` for the existing local subprocess (CPU).

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use crate::stt::protocol::TranscriptionResult;
use crate::stt::status::{SttStatus, SttStatusHandle};
use crate::stt::supervisor::{SttSupervisor, SupervisorError};

pub struct LocalSttBackend {
    supervisor: SttSupervisor,
    status: SttStatusHandle,
    /// Cached value of model_id taken from `SttStatus::Ready`; updated lazily on
    /// each successful transcribe so `backend_id()`/`model_id()` are cheap.
    cached_model_id: Mutex<String>,
}

impl LocalSttBackend {
    pub fn new(supervisor: SttSupervisor) -> Self {
        let status = supervisor.status_handle();
        let cached = match status.get() {
            SttStatus::Ready { model_id, .. } => model_id,
            _ => "unknown".into(),
        };
        Self { supervisor, status, cached_model_id: Mutex::new(cached) }
    }
}

#[async_trait]
impl SttBackend for LocalSttBackend {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError> {
        let r = self
            .supervisor
            .transcribe(
                req.request_id,
                req.samples,
                &req.language,
                &req.initial_prompt,
                req.audio_path,
                req.started_at_ms,
            )
            .await
            .map_err(|e: SupervisorError| BackendError::Worker(e.to_string()))?;
        // Refresh cached_model_id from the latest Ready state.
        if let SttStatus::Ready { model_id, .. } = self.status.get() {
            *self.cached_model_id.lock() = model_id;
        }
        Ok(r)
    }
    fn backend_id(&self) -> &str { "cpu" }
    fn model_id(&self) -> &str {
        // Static return because the trait signature constrains the lifetime.
        // For dynamic model_id we expose `cached_model_id` via a separate
        // helper; callers that need the live value go through `SttStatus`.
        "local-whisper"
    }
}
```

- [ ] **Step 2: Re-export**

`mod.rs`: `pub mod local; pub use local::LocalSttBackend;`

- [ ] **Step 3: `cargo check --workspace` green.**

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/stt/local.rs src-tauri/src/stt/mod.rs
git commit -m "feat(stt): LocalSttBackend adapter over SttSupervisor"
```

---

## Task 5: Keyring wrapper + redaction helper (TDD)

The thinnest possible wrapper around the `keyring` crate, plus a `redact_secret` helper used by every error-path string formatter.

**Files:**
- Create: `src-tauri/src/stt/keyring.rs`
- Modify: `src-tauri/src/stt/mod.rs`

- [ ] **Step 1: Write the unit tests first (in-module `#[cfg(test)] mod tests`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_replaces_full_key() {
        let key = "sk-abcdefghijklmnop";
        let s = format!("Bearer {key}");
        let red = redact_secret(&s, key);
        assert!(!red.contains("abcdef"));
        assert!(red.contains("sk-"));
        assert!(red.contains("****"));
    }

    #[test]
    fn redact_handles_short_keys() {
        let key = "abc";
        let s = "leaked: abc here";
        let red = redact_secret(s, key);
        assert!(!red.contains("abc here"));
    }

    #[test]
    fn redact_noop_on_empty_key() {
        assert_eq!(redact_secret("hello", ""), "hello");
    }
}
```

Run — fails to compile (`redact_secret` doesn't exist yet).

- [ ] **Step 2: Implement the wrapper + redactor**

```rust
//! Thin wrapper around the OS credential store for the OpenAI API key.
//!
//! Windows backs onto Credential Manager (which uses DPAPI under the hood);
//! macOS and Linux are TODO but not in v1 scope.

const SERVICE: &str = "voicetabs";
const KEY_NAME: &str = "openai_api_key";

#[derive(Debug, thiserror::Error)]
pub enum KeyringError {
    #[error("keyring: {0}")]
    Backend(String),
    #[error("no key configured")]
    NotFound,
}

pub fn get_api_key() -> Result<String, KeyringError> {
    let entry = keyring::Entry::new(SERVICE, KEY_NAME)
        .map_err(|e| KeyringError::Backend(e.to_string()))?;
    match entry.get_password() {
        Ok(s) => Ok(s),
        Err(keyring::Error::NoEntry) => Err(KeyringError::NotFound),
        Err(e) => Err(KeyringError::Backend(e.to_string())),
    }
}

pub fn set_api_key(value: &str) -> Result<(), KeyringError> {
    let entry = keyring::Entry::new(SERVICE, KEY_NAME)
        .map_err(|e| KeyringError::Backend(e.to_string()))?;
    entry
        .set_password(value)
        .map_err(|e| KeyringError::Backend(e.to_string()))
}

pub fn clear_api_key() -> Result<(), KeyringError> {
    let entry = keyring::Entry::new(SERVICE, KEY_NAME)
        .map_err(|e| KeyringError::Backend(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // idempotent
        Err(e) => Err(KeyringError::Backend(e.to_string())),
    }
}

/// Mask `key` in `haystack`, keeping the first 4 chars to aid in support
/// debugging (e.g. "sk-a***"). Empty key → no-op.
pub fn redact_secret(haystack: &str, key: &str) -> String {
    if key.is_empty() {
        return haystack.to_string();
    }
    let prefix: String = key.chars().take(4).collect();
    let mask = format!("{prefix}{}", "*".repeat(10));
    haystack.replace(key, &mask)
}
```

- [ ] **Step 3: Re-export**

`mod.rs`: `pub mod keyring;`

- [ ] **Step 4: Run the unit tests**

```powershell
cargo test -p voicetabs stt::keyring::tests
```

The `get_api_key` / `set_api_key` / `clear_api_key` functions are exercised only by **integration** tests on a real OS keyring (manual acceptance Task 17). We do not unit-test against the real Credential Manager from CI.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/stt/keyring.rs src-tauri/src/stt/mod.rs
git commit -m "feat(stt): keyring wrapper + redact_secret helper (TDD)"
```

---

## Task 6: OpenAI backend implementation (TDD with mockito)

The HTTP client, multipart builder, retry loop, error mapping.

**Files:**
- Create: `src-tauri/src/stt/openai.rs`
- Modify: `src-tauri/src/stt/mod.rs`
- Create: `src-tauri/tests/openai_backend_test.rs`

- [ ] **Step 1: Write the integration tests first**

`src-tauri/tests/openai_backend_test.rs`:

```rust
//! Tests the OpenAI backend against a mock HTTP server. Covers happy path,
//! 401 (auth), 429 (rate-limited with retry), and 500 (server error).

use std::sync::Arc;
use voicetabs_lib::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use voicetabs_lib::stt::openai::OpenAiSttBackend;

fn sample_request() -> TranscribeRequest {
    TranscribeRequest {
        request_id: "r1".into(),
        // 0.25 s of zeros at 16 kHz — small enough for fast multipart build,
        // large enough for the WAV writer to produce a valid header.
        samples: vec![0.0_f32; 4_000],
        sample_rate: 16_000,
        language: "pt".into(),
        initial_prompt: String::new(),
        audio_path: None,
        started_at_ms: 0,
    }
}

#[tokio::test]
async fn happy_path_returns_text() {
    let mut server = mockito::Server::new_async().await;
    let _m = server.mock("POST", "/v1/audio/transcriptions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"text":"olá mundo"}"#)
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let r = backend.transcribe(sample_request()).await.unwrap();
    assert_eq!(r.text, "olá mundo");
    assert_eq!(r.avg_logprob, 0.0);
    assert_eq!(r.no_speech_prob, 0.0);
    assert!(r.duration_ms > 0);
}

#[tokio::test]
async fn auth_failure_maps_to_auth_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server.mock("POST", "/v1/audio/transcriptions")
        .with_status(401)
        .with_body(r#"{"error":{"message":"invalid api key"}}"#)
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    assert!(matches!(err, BackendError::Auth));
}

#[tokio::test]
async fn rate_limit_retries_once_then_fails() {
    let mut server = mockito::Server::new_async().await;
    let m = server.mock("POST", "/v1/audio/transcriptions")
        .with_status(429)
        .with_body("rate limited")
        .expect(2) // initial + 1 retry
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    assert!(matches!(err, BackendError::RateLimited));
    m.assert_async().await;
}

#[tokio::test]
async fn rate_limit_then_success() {
    let mut server = mockito::Server::new_async().await;
    let _m_429 = server.mock("POST", "/v1/audio/transcriptions")
        .with_status(429)
        .with_body("rate limited")
        .expect(1)
        .create_async()
        .await;
    let _m_ok = server.mock("POST", "/v1/audio/transcriptions")
        .with_status(200)
        .with_body(r#"{"text":"recovered"}"#)
        .expect(1)
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let r = backend.transcribe(sample_request()).await.unwrap();
    assert_eq!(r.text, "recovered");
}

#[tokio::test]
async fn server_error_maps_to_server_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server.mock("POST", "/v1/audio/transcriptions")
        .with_status(500)
        .with_body("oops")
        .create_async()
        .await;

    let backend = OpenAiSttBackend::new_with_endpoint(
        "sk-test-key".into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    assert!(matches!(err, BackendError::Server(_)));
}

#[tokio::test]
async fn error_strings_never_contain_api_key() {
    let mut server = mockito::Server::new_async().await;
    let _m = server.mock("POST", "/v1/audio/transcriptions")
        .with_status(401)
        .with_body("body that does not contain the key")
        .create_async()
        .await;

    let key = "sk-SECRETSECRET";
    let backend = OpenAiSttBackend::new_with_endpoint(
        key.into(),
        format!("{}/v1/audio/transcriptions", server.url()),
    );
    let err = backend.transcribe(sample_request()).await.unwrap_err();
    let s = format!("{err:?} / {err}");
    assert!(!s.contains("SECRETSECRET"), "leaked key: {s}");
}
```

Run — fails (`OpenAiSttBackend` doesn't exist). Note: tests call a `new_with_endpoint` constructor that lets us point at the mock; production code uses `new()` which hard-codes the OpenAI URL.

- [ ] **Step 2: Implement `openai.rs`**

```rust
//! OpenAI `gpt-4o-mini-transcribe` backend.

use std::io::Cursor;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::stt::backend::{BackendError, SttBackend, TranscribeRequest};
use crate::stt::keyring::redact_secret;
use crate::stt::protocol::TranscriptionResult;

const PROD_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";
const MODEL_ID: &str = "gpt-4o-mini-transcribe";
const RETRY_BASE_MS: u64 = 500;
const REQUEST_TIMEOUT_S: u64 = 30;

pub struct OpenAiSttBackend {
    api_key: SecretString,
    endpoint: String,
    client: reqwest::Client,
}

impl OpenAiSttBackend {
    pub fn new(api_key: String) -> Self {
        Self::new_with_endpoint(api_key, PROD_ENDPOINT.into())
    }

    pub fn new_with_endpoint(api_key: String, endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_S))
            .build()
            .expect("build reqwest client");
        Self { api_key: SecretString::from(api_key), endpoint, client }
    }

    fn build_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::<u8>::new());
        {
            let mut w = hound::WavWriter::new(&mut cursor, spec)
                .expect("build WavWriter");
            for &s in samples {
                let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                w.write_sample(v).expect("write sample");
            }
            w.finalize().expect("finalize wav");
        }
        cursor.into_inner()
    }

    fn scrub(&self, s: String) -> String {
        redact_secret(&s, self.api_key.expose_secret())
    }

    async fn post_once(
        &self,
        req: &TranscribeRequest,
        wav: &[u8],
    ) -> Result<reqwest::Response, BackendError> {
        let part = Part::bytes(wav.to_vec())
            .file_name("utterance.wav")
            .mime_str("audio/wav")
            .map_err(|e| BackendError::Network(self.scrub(e.to_string())))?;
        let form = Form::new()
            .text("model", MODEL_ID.to_string())
            .text("language", req.language.clone())
            .text("prompt", req.initial_prompt.clone())
            .text("response_format", "json".to_string())
            .part("file", part);
        let resp = self
            .client
            .post(&self.endpoint)
            .bearer_auth(self.api_key.expose_secret())
            .multipart(form)
            .send()
            .await
            .map_err(|e| BackendError::Network(self.scrub(e.to_string())))?;
        Ok(resp)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}

#[async_trait]
impl SttBackend for OpenAiSttBackend {
    async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptionResult, BackendError> {
        let started = std::time::Instant::now();
        let wav = Self::build_wav(&req.samples, req.sample_rate);
        let mut attempt: u32 = 0;
        let resp = loop {
            let r = self.post_once(&req, &wav).await?;
            match r.status() {
                s if s.is_success() => break r,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    return Err(BackendError::Auth);
                }
                StatusCode::TOO_MANY_REQUESTS if attempt == 0 => {
                    attempt += 1;
                    let backoff = Duration::from_millis(RETRY_BASE_MS + 250);
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                StatusCode::TOO_MANY_REQUESTS => return Err(BackendError::RateLimited),
                s if s.is_server_error() => {
                    let body = r.text().await.unwrap_or_default();
                    return Err(BackendError::Server(self.scrub(format!("{s}: {body}"))));
                }
                s => {
                    let body = r.text().await.unwrap_or_default();
                    return Err(BackendError::Server(self.scrub(format!("unexpected {s}: {body}"))));
                }
            }
        };
        let body: OpenAiTranscriptionResponse = resp
            .json()
            .await
            .map_err(|e| BackendError::Server(self.scrub(e.to_string())))?;
        Ok(TranscriptionResult {
            request_id: req.request_id,
            text: body.text,
            avg_logprob: 0.0,
            no_speech_prob: 0.0,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
    fn backend_id(&self) -> &str { "openai" }
    fn model_id(&self) -> &str { MODEL_ID }
}
```

- [ ] **Step 3: Re-export from `mod.rs`** and run the tests.

```powershell
cargo test -p voicetabs --test openai_backend_test
```

All six pass.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/stt/openai.rs src-tauri/src/stt/mod.rs src-tauri/tests/openai_backend_test.rs
git commit -m "feat(stt): OpenAI backend with multipart upload + retry + key redaction"
```

---

## Task 7: `ActiveBackend` hot-swap handle

The `Arc<TokioRwLock<Arc<dyn SttBackend>>>` plus a `set_backend(kind)` helper.

**Files:**
- Create: `src-tauri/src/stt/active.rs`
- Modify: `src-tauri/src/stt/mod.rs`

- [ ] **Step 1: Implement**

```rust
//! Hot-swappable active backend handle.

use std::sync::Arc;

use tokio::sync::RwLock as TokioRwLock;

use crate::stt::backend::SttBackend;

#[derive(Clone)]
pub struct ActiveBackend {
    inner: Arc<TokioRwLock<Arc<dyn SttBackend>>>,
}

impl ActiveBackend {
    pub fn new(initial: Arc<dyn SttBackend>) -> Self {
        Self { inner: Arc::new(TokioRwLock::new(initial)) }
    }

    /// Snapshot the current backend Arc. Short read lock; caller drops it
    /// before awaiting on the returned backend's `transcribe`.
    pub async fn snapshot(&self) -> Arc<dyn SttBackend> {
        self.inner.read().await.clone()
    }

    /// Replace the active backend. Brief write lock.
    pub async fn replace(&self, new: Arc<dyn SttBackend>) {
        *self.inner.write().await = new;
    }
}
```

- [ ] **Step 2: Inline unit test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::backend::{BackendError, SttBackend, TranscribeRequest};
    use crate::stt::protocol::TranscriptionResult;
    use async_trait::async_trait;

    struct Tagged(&'static str);
    #[async_trait]
    impl SttBackend for Tagged {
        async fn transcribe(&self, req: TranscribeRequest) -> Result<TranscriptionResult, BackendError> {
            Ok(TranscriptionResult { request_id: req.request_id, text: self.0.into(), avg_logprob: 0.0, no_speech_prob: 0.0, duration_ms: 1 })
        }
        fn backend_id(&self) -> &str { self.0 }
        fn model_id(&self) -> &str { "m" }
    }

    #[tokio::test]
    async fn replace_swaps_for_next_call() {
        let h = ActiveBackend::new(Arc::new(Tagged("a")));
        let snap = h.snapshot().await;
        let r1 = snap.transcribe(TranscribeRequest {
            request_id: "1".into(), samples: vec![], sample_rate: 16_000,
            language: "pt".into(), initial_prompt: String::new(),
            audio_path: None, started_at_ms: 0,
        }).await.unwrap();
        assert_eq!(r1.text, "a");

        h.replace(Arc::new(Tagged("b"))).await;
        let snap2 = h.snapshot().await;
        let r2 = snap2.transcribe(TranscribeRequest {
            request_id: "2".into(), samples: vec![], sample_rate: 16_000,
            language: "pt".into(), initial_prompt: String::new(),
            audio_path: None, started_at_ms: 0,
        }).await.unwrap();
        assert_eq!(r2.text, "b");

        // The pre-swap snapshot still points at the old backend.
        let r3 = snap.transcribe(TranscribeRequest {
            request_id: "3".into(), samples: vec![], sample_rate: 16_000,
            language: "pt".into(), initial_prompt: String::new(),
            audio_path: None, started_at_ms: 0,
        }).await.unwrap();
        assert_eq!(r3.text, "a", "old snapshot must not see new backend");
    }
}
```

Run: `cargo test -p voicetabs stt::active::tests`. Both pass.

- [ ] **Step 3: Re-export + commit**

```powershell
git add src-tauri/src/stt/active.rs src-tauri/src/stt/mod.rs
git commit -m "feat(stt): ActiveBackend hot-swap handle"
```

---

## Task 8: Backend selector / settings persistence (server side)

The bridge between the `stt_backend` setting and the `ActiveBackend` handle. Pure function — no Tauri, no I/O beyond keyring reads.

**Files:**
- Modify: `src-tauri/src/stt/active.rs` (extend with `BackendKind` + `build_backend`)

- [ ] **Step 1: Add `BackendKind` enum and a builder**

Append to `active.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Local,
    Openai,
}

impl BackendKind {
    pub fn from_setting(s: &str) -> Self {
        match s {
            "openai" => BackendKind::Openai,
            _ => BackendKind::Local,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self { BackendKind::Local => "local", BackendKind::Openai => "openai" }
    }
}

/// Build a fresh `Arc<dyn SttBackend>` for `kind`. For `Openai` this reads
/// the API key from the OS keyring; missing key → `Err(BackendError::MissingApiKey)`.
pub fn build_backend(
    kind: BackendKind,
    local: Arc<dyn SttBackend>,
) -> Result<Arc<dyn SttBackend>, crate::stt::backend::BackendError> {
    use crate::stt::backend::BackendError;
    use crate::stt::keyring as kr;
    use crate::stt::openai::OpenAiSttBackend;
    match kind {
        BackendKind::Local => Ok(local),
        BackendKind::Openai => {
            let key = kr::get_api_key().map_err(|_| BackendError::MissingApiKey)?;
            Ok(Arc::new(OpenAiSttBackend::new(key)))
        }
    }
}
```

- [ ] **Step 2: Unit test the kind parser** (in the same `tests` module)

```rust
#[test]
fn kind_parses_known_values() {
    assert_eq!(BackendKind::from_setting("local"), BackendKind::Local);
    assert_eq!(BackendKind::from_setting("openai"), BackendKind::Openai);
    assert_eq!(BackendKind::from_setting("garbage"), BackendKind::Local);
}
```

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/stt/active.rs
git commit -m "feat(stt): BackendKind + build_backend selector"
```

---

## Task 9: Tauri commands for backend + API key

The frontend-facing API. Stateless wrappers; all business logic is in `stt::active` and `stt::keyring`.

**Files:**
- Create: `src-tauri/src/commands/backend.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: Implement the four commands**

```rust
use std::sync::Arc;

use tauri::State;

use crate::db::{settings as settings_repo, Db};
use crate::stt::active::{build_backend, ActiveBackend, BackendKind};
use crate::stt::backend::{BackendError, SttBackend};
use crate::stt::keyring as kr;
use crate::stt::local::LocalSttBackend;
use crate::stt::status::{SttStatus, SttStatusHandle};

use super::tabs::CommandError;

const SETTING_BACKEND: &str = "stt_backend";
const SETTING_KEY_SET: &str = "openai_api_key_set";

fn cmd_err(code: &str, msg: impl Into<String>) -> CommandError {
    CommandError { code: code.into(), message: msg.into() }
}

#[tauri::command]
pub fn backend_get(db: State<'_, Db>) -> Result<String, CommandError> {
    let s = settings_repo::get::<String>(&db, SETTING_BACKEND)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?
        .unwrap_or_else(|| "local".into());
    Ok(s)
}

#[tauri::command]
pub async fn backend_set(
    kind: String,
    db: State<'_, Db>,
    active: State<'_, ActiveBackend>,
    local_backend: State<'_, Arc<LocalSttBackend>>,
    status: State<'_, SttStatusHandle>,
) -> Result<(), CommandError> {
    let db = db.inner().clone();
    let active = active.inner().clone();
    let local: Arc<dyn SttBackend> = local_backend.inner().clone();
    let status = status.inner().clone();

    let bk = BackendKind::from_setting(&kind);
    let new = build_backend(bk, local).map_err(|e: BackendError| match e {
        BackendError::MissingApiKey => cmd_err(
            "OPENAI_NO_KEY",
            "configure an OpenAI API key before selecting this backend",
        ),
        other => cmd_err("BACKEND_BUILD_ERROR", other.to_string()),
    })?;
    active.replace(new).await;
    settings_repo::set(&db, SETTING_BACKEND, &bk.as_str().to_string())
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;
    status.set(SttStatus::Ready {
        backend: bk.as_str().into(),
        model_id: match bk {
            BackendKind::Local => match status.get() {
                SttStatus::Ready { model_id, .. } => model_id,
                _ => "local-whisper".into(),
            },
            BackendKind::Openai => "gpt-4o-mini-transcribe".into(),
        },
    });
    Ok(())
}

#[tauri::command]
pub fn openai_key_set(value: String, db: State<'_, Db>) -> Result<(), CommandError> {
    if value.trim().is_empty() {
        return Err(cmd_err("EMPTY_KEY", "API key must not be empty"));
    }
    kr::set_api_key(&value).map_err(|e| cmd_err("KEYRING_ERROR", e.to_string()))?;
    settings_repo::set(&db, SETTING_KEY_SET, &true)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn openai_key_clear(
    db: State<'_, Db>,
    active: State<'_, ActiveBackend>,
    local_backend: State<'_, Arc<LocalSttBackend>>,
    status: State<'_, SttStatusHandle>,
) -> Result<(), CommandError> {
    let db = db.inner().clone();
    let active = active.inner().clone();
    let local: Arc<dyn SttBackend> = local_backend.inner().clone();
    let status = status.inner().clone();

    kr::clear_api_key().map_err(|e| cmd_err("KEYRING_ERROR", e.to_string()))?;
    settings_repo::set(&db, SETTING_KEY_SET, &false)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;

    // If OpenAI was the active backend, fall back to Local automatically.
    let current_setting = settings_repo::get::<String>(&db, SETTING_BACKEND)
        .ok()
        .flatten()
        .unwrap_or_else(|| "local".into());
    if current_setting == "openai" {
        settings_repo::set(&db, SETTING_BACKEND, &"local".to_string())
            .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?;
        active.replace(local).await;
        let model_id = match status.get() {
            SttStatus::Ready { model_id, .. } => model_id,
            _ => "local-whisper".into(),
        };
        status.set(SttStatus::Ready { backend: "cpu".into(), model_id });
    }
    Ok(())
}

#[tauri::command]
pub fn openai_key_status(db: State<'_, Db>) -> Result<bool, CommandError> {
    let v: bool = settings_repo::get(&db, SETTING_KEY_SET)
        .map_err(|e| cmd_err("SETTINGS_ERROR", e.to_string()))?
        .unwrap_or(false);
    Ok(v)
}
```

- [ ] **Step 2: Register in `commands/mod.rs`**

```rust
pub mod backend;
```

(plus the existing modules).

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/commands/backend.rs src-tauri/src/commands/mod.rs
git commit -m "feat(commands): backend_get/set + openai_key_set/clear/status"
```

---

## Task 10: Wire it all up in `lib.rs`

Replace the supervisor-direct calls with `ActiveBackend`-routed calls; remove the hardcoded `"cpu"` literal; register the new state slots and handlers.

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/segments.rs`

- [ ] **Step 1: In `lib.rs::run()`, derive the initial backend kind from settings**

Replace the hardcoded `let backend = "cpu".to_string();` block with:

```rust
let initial_kind: BackendKind = db::settings::get::<String>(&db, "stt_backend")
    .ok()
    .flatten()
    .map(|s| BackendKind::from_setting(&s))
    .unwrap_or(BackendKind::Local);
let backend_str = initial_kind.as_str().to_string();

let stt_status = SttStatusHandle::new(SttStatus::Loading { backend: backend_str.clone() });
let stt_status_for_state = stt_status.clone();
```

(Imports: `use crate::stt::active::{ActiveBackend, BackendKind, build_backend};` and `use crate::stt::local::LocalSttBackend;`)

- [ ] **Step 2: Construct supervisor as before, wrap in LocalSttBackend, and seed ActiveBackend**

Inside the `.setup` closure, after `let supervisor = SttSupervisor::new(cfg, stt_status.clone());` and `app.manage(supervisor.clone());`, add:

```rust
let local_backend: Arc<LocalSttBackend> = Arc::new(LocalSttBackend::new(supervisor.clone()));
let initial_arc: Arc<dyn crate::stt::backend::SttBackend> = match build_backend(initial_kind, local_backend.clone()) {
    Ok(b) => b,
    Err(e) => {
        tracing::warn!("initial backend build failed ({e}); falling back to local");
        local_backend.clone()
    }
};
let active = ActiveBackend::new(initial_arc);
app.manage(active.clone());
app.manage(local_backend.clone());
```

- [ ] **Step 3: Replace the startup banner**

Drop the hardcoded `STT BACKEND: CPU (local)` line. Replace with:

```rust
tracing::warn!(
    "\n\
    ═══════════════════════════════════════════════════════════════\n\
      STT BACKEND:   {}\n\
      Worker binary: {}\n\
      Model:         {} ({:.0} MB)\n\
    ═══════════════════════════════════════════════════════════════",
    match initial_kind { BackendKind::Local => "Local (CPU)", BackendKind::Openai => "OpenAI (cloud)" },
    worker_binary.display(),
    model_name,
    model_size_mb,
);
```

- [ ] **Step 4: Register the new commands**

In the `invoke_handler!` macro list add:

```rust
commands::backend::backend_get,
commands::backend::backend_set,
commands::backend::openai_key_set,
commands::backend::openai_key_clear,
commands::backend::openai_key_status,
```

- [ ] **Step 5: Switch the drainer to call `ActiveBackend`**

In `run_segment_pipeline`, replace `sup.transcribe(...)` with:

```rust
let backend = active.snapshot().await;
let result = match backend.transcribe(crate::stt::backend::TranscribeRequest {
    request_id: request_id.clone(),
    samples,
    sample_rate: 16_000,
    language: language.clone(),
    initial_prompt: initial_prompt.clone(),
    audio_path: Some(audio_path.clone()),
    started_at_ms,
}).await { … }
```

Pass `active: ActiveBackend` into `drain_utterances` / `run_segment_pipeline` instead of the supervisor. (Keep the supervisor managed for crash-recovery semantics — `LocalSttBackend` holds a clone.)

The `model_id` resolution becomes `backend.model_id().to_string()` when the active backend is OpenAI; for Local, keep reading from `SttStatusHandle` (because the supervisor's model_id is dynamic). Compose:

```rust
let model_id = if backend.backend_id() == "openai" {
    backend.model_id().to_string()
} else {
    match sup.status_handle().get() {
        SttStatus::Ready { model_id, .. } => model_id,
        _ => "unknown".into(),
    }
};
```

- [ ] **Step 6: Apply the same change to `commands/segments.rs::segments_retranscribe`**

It takes `stt: State<'_, SttSupervisor>` today; add `active: State<'_, ActiveBackend>` and switch the `supervisor.transcribe(...)` call to `active.snapshot().await.transcribe(req).await`. Keep `stt_status` and `stt: State<'_, SttSupervisor>` references for the model_id-from-status fallback only — or, simpler, drop them and use `backend.model_id()` for OpenAI and read status for Local exactly as in Step 5.

- [ ] **Step 7: `cargo check --workspace` is green and all existing tests still pass.**

```powershell
cargo test --workspace
```

- [ ] **Step 8: Commit**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/commands/segments.rs
git commit -m "feat(stt): route segment pipeline through ActiveBackend trait handle"
```

---

## Task 11: Frontend Tauri bindings

Add the `backendApi` to `src/lib/tauri.ts`.

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Append**

```ts
export type BackendKind = "local" | "openai";

export const backendApi = {
  get(): Promise<BackendKind> {
    return invoke<BackendKind>("backend_get");
  },
  set(kind: BackendKind): Promise<void> {
    return invoke<void>("backend_set", { kind });
  },
  openaiKeyStatus(): Promise<boolean> {
    return invoke<boolean>("openai_key_status");
  },
  openaiKeySet(value: string): Promise<void> {
    return invoke<void>("openai_key_set", { value });
  },
  openaiKeyClear(): Promise<void> {
    return invoke<void>("openai_key_clear");
  },
};
```

- [ ] **Step 2: `npm run typecheck` green.**

- [ ] **Step 3: Commit**

```powershell
git add src/lib/tauri.ts
git commit -m "feat(frontend): backendApi for stt backend selection"
```

---

## Task 12: Settings store + i18n keys

**Files:**
- Modify: `src/stores/settingsStore.ts`
- Modify: `src/i18n/locales/pt-BR.json`
- Modify: `src/i18n/locales/en.json`

- [ ] **Step 1: Extend the store**

Add fields and actions:

```ts
backend: BackendKind;
openaiKeySet: boolean;
loadBackend: () => Promise<void>;
setBackend: (kind: BackendKind) => Promise<void>;
saveOpenAiKey: (value: string) => Promise<void>;
clearOpenAiKey: () => Promise<void>;
```

In `load()`, after the vocab block:

```ts
const [backend, openaiKeySet] = await Promise.all([
  backendApi.get(),
  backendApi.openaiKeyStatus(),
]);
set({ backend, openaiKeySet });
```

Actions:
```ts
async setBackend(kind) {
  await backendApi.set(kind);
  set({ backend: kind });
},
async saveOpenAiKey(value) {
  await backendApi.openaiKeySet(value);
  set({ openaiKeySet: true });
},
async clearOpenAiKey() {
  await backendApi.openaiKeyClear();
  // The backend command auto-reverts to local when clearing while openai is active.
  const backend = await backendApi.get();
  set({ openaiKeySet: false, backend });
},
```

- [ ] **Step 2: i18n keys (PT-BR)**

Add under `"settings"`:

```json
"backend": {
  "heading": "Backend de transcrição",
  "local": "Local (CPU)",
  "openai": "OpenAI (cloud)",
  "apiKey": "Chave da API OpenAI",
  "apiKeyPlaceholder": "sk-…",
  "save": "Salvar chave",
  "clear": "Limpar chave",
  "configured": "Chave configurada ✓",
  "notConfigured": "Nenhuma chave configurada",
  "disclaimer": "Áudio enviado para api.openai.com. Veja a política de privacidade da OpenAI.",
  "errorNoKey": "Configure a chave da OpenAI antes de selecionar este backend."
}
```

Mirror in `en.json` with English copy.

- [ ] **Step 3: TS typecheck green.**

- [ ] **Step 4: Commit**

```powershell
git add src/stores/settingsStore.ts src/i18n/locales/pt-BR.json src/i18n/locales/en.json
git commit -m "feat(frontend): settings store + i18n for backend + openai key"
```

---

## Task 13: `BackendSettings` React component (TDD)

**Files:**
- Create: `src/components/BackendSettings.tsx`
- Create: `src/__tests__/BackendSettings.test.tsx`
- Modify: `src/components/SettingsDrawer.tsx`

- [ ] **Step 1: Write the tests first**

`src/__tests__/BackendSettings.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

import { BackendSettings } from "../components/BackendSettings";
import { useSettingsStore } from "../stores/settingsStore";

vi.mock("../lib/tauri", () => ({
  backendApi: {
    get: vi.fn().mockResolvedValue("local"),
    set: vi.fn().mockResolvedValue(undefined),
    openaiKeyStatus: vi.fn().mockResolvedValue(false),
    openaiKeySet: vi.fn().mockResolvedValue(undefined),
    openaiKeyClear: vi.fn().mockResolvedValue(undefined),
  },
  settingsApi: { get: vi.fn().mockResolvedValue(null), set: vi.fn() },
}));

describe("BackendSettings", () => {
  beforeEach(() => {
    useSettingsStore.setState({ backend: "local", openaiKeySet: false });
  });

  it("renders both radio options and the local one selected by default", () => {
    render(<BackendSettings />);
    expect(screen.getByLabelText(/Local/i)).toBeChecked();
    expect(screen.getByLabelText(/OpenAI/i)).not.toBeChecked();
  });

  it("shows the API key input when OpenAI is selected", () => {
    useSettingsStore.setState({ backend: "openai", openaiKeySet: false });
    render(<BackendSettings />);
    expect(screen.getByPlaceholderText(/sk-/i)).toBeInTheDocument();
    expect(screen.getByText(/Nenhuma chave configurada|Not configured/i)).toBeInTheDocument();
  });

  it("saves an entered key and shows configured state", async () => {
    useSettingsStore.setState({ backend: "openai", openaiKeySet: false });
    render(<BackendSettings />);
    const input = screen.getByPlaceholderText(/sk-/i) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "sk-1234567890" } });
    fireEvent.click(screen.getByRole("button", { name: /Salvar|Save/i }));
    await waitFor(() => expect(useSettingsStore.getState().openaiKeySet).toBe(true));
    expect(input.value).toBe("");
  });
});
```

Run — fails (component missing).

- [ ] **Step 2: Implement the component**

```tsx
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { BackendKind } from "../lib/tauri";
import { useSettingsStore } from "../stores/settingsStore";

export function BackendSettings() {
  const { t } = useTranslation();
  const { backend, openaiKeySet, setBackend, saveOpenAiKey, clearOpenAiKey } = useSettingsStore();
  const [keyDraft, setKeyDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function pickBackend(kind: BackendKind) {
    setError(null);
    try {
      await setBackend(kind);
    } catch (e: unknown) {
      const msg = (e as { message?: string })?.message ?? String(e);
      setError(msg.includes("OPENAI_NO_KEY") ? t("settings.backend.errorNoKey") : msg);
    }
  }

  async function onSaveKey() {
    if (!keyDraft.trim()) return;
    await saveOpenAiKey(keyDraft.trim());
    setKeyDraft("");
  }

  return (
    <section className="drawer__section" data-testid="backend-settings">
      <h3>{t("settings.backend.heading")}</h3>
      <label>
        <input
          type="radio"
          name="backend"
          value="local"
          checked={backend === "local"}
          onChange={() => void pickBackend("local")}
        />
        {t("settings.backend.local")}
      </label>
      <label>
        <input
          type="radio"
          name="backend"
          value="openai"
          checked={backend === "openai"}
          onChange={() => void pickBackend("openai")}
        />
        {t("settings.backend.openai")}
      </label>

      {backend === "openai" && (
        <div className="backend__openai">
          <label htmlFor="openai-key">{t("settings.backend.apiKey")}</label>
          <input
            id="openai-key"
            type="password"
            placeholder={t("settings.backend.apiKeyPlaceholder")}
            value={keyDraft}
            onChange={(e) => setKeyDraft(e.target.value)}
            autoComplete="off"
          />
          <div className="backend__key-actions">
            <button type="button" onClick={() => void onSaveKey()}>
              {t("settings.backend.save")}
            </button>
            <button type="button" onClick={() => void clearOpenAiKey()}>
              {t("settings.backend.clear")}
            </button>
          </div>
          <p className={openaiKeySet ? "backend__status--ok" : "backend__status--missing"}>
            {openaiKeySet ? t("settings.backend.configured") : t("settings.backend.notConfigured")}
          </p>
          <p className="backend__disclaimer">{t("settings.backend.disclaimer")}</p>
        </div>
      )}
      {error && <p role="alert" className="backend__error">{error}</p>}
    </section>
  );
}
```

- [ ] **Step 3: Mount it inside `SettingsDrawer.tsx`** (just below `<VocabSettings />`).

```tsx
import { BackendSettings } from "./BackendSettings";
…
<VocabSettings />
<BackendSettings />
```

- [ ] **Step 4: Run tests**

```powershell
npm test -- BackendSettings
```

All three pass.

- [ ] **Step 5: Commit**

```powershell
git add src/components/BackendSettings.tsx src/components/SettingsDrawer.tsx src/__tests__/BackendSettings.test.tsx
git commit -m "feat(frontend): BackendSettings drawer section (TDD)"
```

---

## Task 14: Extend `settingsStore` tests

**Files:**
- Modify: `src/__tests__/settingsStore.test.ts` (or create if missing)

- [ ] **Step 1: Tests cover**
  - `load()` populates `backend` and `openaiKeySet`
  - `setBackend("openai")` calls `backendApi.set` and updates store
  - `saveOpenAiKey("sk-x")` flips `openaiKeySet` to true
  - `clearOpenAiKey()` flips `openaiKeySet` to false AND re-reads `backend` (covers the auto-revert)

Use the same `vi.mock("../lib/tauri", …)` pattern as the existing test.

- [ ] **Step 2: `npm test` green.**

- [ ] **Step 3: Commit**

```powershell
git add src/__tests__/settingsStore.test.ts
git commit -m "test(frontend): settingsStore covers backend + openai-key flows"
```

---

## Task 15: Full test sweep + clippy

- [ ] **Step 1: Rust**

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 2: Frontend**

```powershell
npm run typecheck
npm test
```

- [ ] **Step 3: Counts check**

Expect ≥ 73 Rust lib tests still passing (no regressions) plus ~10 new ones across `stt_backend_trait_test`, `openai_backend_test`, `stt::keyring::tests`, `stt::active::tests`. Expect ≥ 33 TS tests plus ~3 new ones.

- [ ] **Step 4: Commit any clippy fixes**

```powershell
git add -u
git commit -m "chore: clippy cleanup for Phase 7"
```

(Skip if no changes.)

---

## Task 16: (Stretch) Streaming via SSE — DO NOT IMPLEMENT IN v1

Documenting this here so future work has a starting point.

- `gpt-4o-mini-transcribe` supports `"stream": true` returning a `text/event-stream` of `transcript.text.delta` and `transcript.text.done` events.
- Implementation sketch:
  - Add an `OpenAiSttStreamingBackend` (or extend `OpenAiSttBackend` with a `streaming: bool` flag).
  - Read SSE chunks with `reqwest::Response::bytes_stream()`; parse `data: {…}\n\n` frames; on `done`, return the full text.
  - Stretch UI: pre-final placeholder card in the segment list that updates as deltas arrive (would violate **L2** "no partial / streaming UI" unless we render only on `done`).
- Decision: not in v1. The non-streaming path already meets the design goals; streaming is purely a latency optimisation that adds UI-state complexity. Revisit after we measure typical cloud-path latency in real use.

---

## Task 17: Manual acceptance — happy path + swap + invalid key

Last task. Drive every acceptance criterion manually on Windows 11.

- [ ] **A-P7-1 (default offline path)** — Fresh install, no keyring entry. Capture, speak PT-BR, see segment.
- [ ] **A-P7-2 (key storage)** — Save key via drawer. `cmdkey /list:voicetabs*` shows the entry. `Get-Content $env:APPDATA\voicetabs\logs\voicetabs.log | Select-String "sk-"` → no matches. `sqlite3 $env:APPDATA\voicetabs\voicetabs.db "select value from settings where key='openai_api_key_set';"` → `true`. The same query for `openai_api_key` returns nothing.
- [ ] **A-P7-3 (cloud happy path)** — Select OpenAI, speak, observe segment + status dot title shows OpenAI.
- [ ] **A-P7-4 (mid-session swap)** — Local → speak → switch → speak. Both segments present. Inspect `segments.model_id` for each: first row contains the whisper file name; second contains `gpt-4o-mini-transcribe`.
- [ ] **A-P7-5 (invalid key)** — Save `sk-invalid`. Switch to OpenAI. Speak → no segment, red dot, log line shows masked `sk-i**********` not the full key.
- [ ] **A-P7-6 (clear key reverts backend)** — From the OpenAI-selected state, click Clear. Radio resets to Local, status dot green, `cmdkey /list:voicetabs*` no longer shows the entry, `openai_api_key_set` flips to `false`.
- [ ] **A-P7-7 (test suites)** — `cargo test --workspace` green, `npm test` green, `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] **A-P7-8 (CI)** — Push, observe master CI run green.

- [ ] **Step 99: Update spec status note** if any acceptance criterion landed differently than planned (e.g. the auto-revert flow). No code change; comment in this plan file noting the deviation.

---

## Self-review summary

- API key is never logged: enforced in `openai.rs` via `scrub` on every error string + `SecretString` for the in-memory holder (Task 6).
- Trait is `Send + Sync`: declared in `backend.rs` (Task 3) and round-tripped through `Arc<dyn SttBackend>` in `stt_backend_trait_test.rs`.
- Mid-session swap doesn't drop in-flight utterances: `ActiveBackend::snapshot` returns the old `Arc` so the in-flight call resolves against the old backend even after `replace()` (asserted in `stt::active::tests::replace_swaps_for_next_call`).
- `keyring` 3.x verified compatible with Windows 11 (Credential Manager / DPAPI): confirmed via the crate README's Win11 testing matrix.
- WAV multipart upload built in memory with `Part::bytes()` (no tempfile): see `OpenAiSttBackend::build_wav` + `post_once` in Task 6.
