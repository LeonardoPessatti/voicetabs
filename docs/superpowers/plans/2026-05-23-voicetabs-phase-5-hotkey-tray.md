# VoiceTabs — Phase 5 (Hotkey + Tray + Capture Modes) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Phase 4 dependency.** This plan can only START when Phase 4 lands on `master`. Phase 4 delivers the working pipeline (`CaptureController` worker thread, `SttSupervisor`, `segment-created` events, `SettingsDrawer.tsx` with the vocab section). Phase 5 strictly adds capture-mode gating, two new hotkey backends, and a system tray. It modifies the controller worker loop in ONE place (a new `HotkeyEvent` arm in `select!`) and otherwise lives in new modules. If `CaptureController::spawn` no longer takes the same arguments described in §"Architecture" below, **STOP** and reconcile before continuing.

**Goal:** Push-to-talk works from anywhere on the system (window minimized, app unfocused, any monitor) using either a global keyboard shortcut or a mouse side-button. The user can switch between always-on and PTT modes from settings without restarting. Closing the window minimizes to a tray icon that left-click toggles back. Acceptance criterion A4 is met end-to-end.

**Architecture:** Three new subsystems land in `src-tauri/`, all behind small `Send + Sync` handles held in Tauri's `manage()` slots so commands and the capture worker can reach them:

1. **`HotkeyManager`** — owns two backends behind a trait. The **`KeyboardBackend`** wraps `tauri-plugin-global-shortcut` v2 (`ShortcutState::Pressed` → `on_press`, `ShortcutState::Released` → `on_release`). The **`MouseBackend`** owns a dedicated OS thread that installs `SetWindowsHookExW(WH_MOUSE_LL, …)` via the `windows` crate, pumps the message loop, and forwards `WM_XBUTTONDOWN`/`WM_XBUTTONUP` (and any other bound mouse button) to a crossbeam channel. The hook is **strictly read-only** — we never modify `MSLLHOOKSTRUCT` and always return `CallNextHookEx(...)`. The manager exposes `set_binding(Binding) -> Result`, `clear_binding()`, `subscribe() -> Receiver<HotkeyEvent>`, and `capture_next_press(Duration) -> Result<Binding>`. Internally a `parking_lot::RwLock<Option<Binding>>` mediates current state; only the active binding's backend fires events on the wire.

2. **`CaptureMode`** — a `Copy` enum (`AlwaysOn`, `Ptt`) backed by `settings.capture_mode`. A `Arc<RwLock<CaptureMode>>` is mounted on Tauri state; `commands::settings::settings_set` notices when the key is `"capture_mode"` and updates the lock in place (no controller restart). The capture worker reads the mode at every relevant decision point: rising-edge VAD events (ignored in PTT), falling-edge VAD events (ignored in PTT), and hotkey press/release events (only honored in PTT).

3. **`Tray`** — built in `setup()` using `tauri-plugin-tray-icon`. (If by the time this plan runs Tauri 2 has folded tray support into core under `tauri::tray::TrayIconBuilder`, prefer that — the API surface we use is the same and no plugin install is needed. The MouseBackend is unchanged either way.) The tray owns an icon (green / gray PNG bytes baked into the binary), a context menu (Show/Hide, Capture toggle, Mode toggle, Quit), and intercepts window close events: the main window's `on_window_event` swaps a Close event for `window.hide()` unless a `should_exit: AtomicBool` flag was set by the Quit menu item.

The data flow for one PTT utterance is now: hotkey press → `HotkeyEvent::Press` lands on the controller worker's `select!` → worker checks mode (`Ptt`); if so, force `VadStateMachine` into `Speaking` and snapshot `(start_tab_id, vocab_snapshot, language)` exactly like a rising edge would → frames stream into the `UtteranceBuilder` → hotkey release → `HotkeyEvent::Release` → worker forces `VadStateMachine` back to `Idle` and calls `UtteranceBuilder::on_vad_event(FallingEdge { … })` to finalize → identical Phase-4 drainer path from there (STT → RMS → filter → insert → emit). In `AlwaysOn`, hotkey events are **dropped on the floor** and VAD edges drive the boundaries (Phase 4 behavior, unchanged).

**Tech Stack additions:**

| Crate / package | Why |
|---|---|
| `tauri-plugin-global-shortcut = "2"` | Press + release events for global keyboard shortcuts. |
| `tauri-plugin-tray-icon = "2"` (or `tauri::tray` if folded into core) | System tray icon + menu. |
| `windows = { version = "0.58", features = ["Win32_UI_WindowsAndMessaging", "Win32_System_LibraryLoader", "Win32_Foundation"] }` | `SetWindowsHookExW`, `GetMessageW`, `CallNextHookEx`, `GetModuleHandleW`. |
| `@tauri-apps/plugin-global-shortcut` | JS-side init of the plugin (the React app calls nothing — the plugin lives on the Rust side — but the plugin's `init()` must be registered in `main.tsx`). |

No new TS dependencies beyond the global-shortcut plugin. `react-i18next`, Zustand, and the existing Tauri JS API are sufficient.

**Reference spec:** `docs/superpowers/specs/2026-05-20-voicetabs-design.md` — focus on §7.6 (HotkeyManager), §7.8 (system tray), §7.11 (capture modes), plus §5.2 step 2 (PTT release forces Speaking → Idle).

**Builds on:** Phase 0+1 foundation, Phase 2 audio+VAD, Phase 3 STT subprocess, Phase 4 segments. Touches Phase 4's `CaptureController` worker loop in one place; otherwise additive.

---

## Acceptance for this plan

- **A4 (full).** Bind a keyboard shortcut (e.g. `RightControl`). Switch the app to PTT. Click the OS minimize button OR alt-tab to another app. Hold the bound key, speak a sentence, release. Within ≤ 1.5 s a segment card appears under the active tab. Repeat with a mouse side button (MB4/MB5/X1/X2) bound instead.
- Mode switch in Settings → Captura is hot-reloadable: changing from "Sempre ativo" to "Push-to-talk" while capture is ON immediately stops VAD-driven boundaries; subsequent hotkey presses drive utterances. Changing back resumes VAD-driven capture.
- "Bind hotkey" capture: clicking **Vincular** in Settings → Captura puts the app in capture mode; the very next physical key OR mouse button press (whichever fires first) is recorded as the new binding, displayed as `"Right Ctrl"` / `"Mouse Button 5"`, and persisted. Pressing **Esc** during capture aborts without changing the binding.
- Tray icon visible at all times after startup. Left-click toggles the main window (`hide`/`show`+focus). Menu items work:
  - **Mostrar/Ocultar** (i18n: `tray.show` / `tray.hide`) toggles window visibility.
  - **Captura: on/off** toggles capture (same effect as the footer button).
  - **Modo: sempre ativo / PTT** toggles between modes (same effect as the dropdown in settings).
  - **Sair** (`tray.quit`) exits the process (sets `should_exit = true`, then `app.exit(0)`).
- Closing the main window via the OS × button **hides to tray** (window remains hidden, process keeps running). Re-clicking the tray icon or "Mostrar" restores the window.
- Tray icon color reflects capture state: green when capturing, gray when idle.
- `cargo test` is green; **+ ~22 new Rust unit tests** land in this phase (`HotkeyManager` binding state, `Binding` parse/display, `CaptureMode` settings round-trip, mock backend dispatch, capture-worker PTT gating).
- `npm test` is green; **+ ~10 new TS tests** (CaptureModeSelector, HotkeyBinder including Esc-cancels, settingsStore PTT round-trip).
- L1 (≤ 1.5 s end-to-end) and L2 (no flicker) are inherited from Phase 4; we do not regress them. Hotkey delivery is well under 5 ms (Win32 hook + crossbeam send + `select!` recv), so it does not threaten L1.

## Out of scope (deferred to later plans)

- **Multi-key chord bindings** (e.g. `Ctrl+Shift+Space`). v1 binds exactly one key OR one mouse button. The plugin supports chords but the capture UI does not.
- **Per-window or per-app hotkey gating** (e.g. "PTT only when this app is focused"). The whole point of A4 is system-wide.
- **Tray icon animation** (pulse while capturing). Static green/gray is enough.
- **Multiple bindings** (e.g. two PTT keys). Single binding only.
- **Hotkey conflicts with other apps.** The global-shortcut plugin will fail to register if another app already owns the chord; we surface the error in a toast and ask the user to pick another binding. We do not try to force-acquire.
- **Mouse-wheel or middle-button bindings.** XBUTTON1, XBUTTON2, plus left/right/middle clicks are *technically* feasible through `WH_MOUSE_LL`, but binding the left mouse button would brick the UI. We allow only XBUTTON1 / XBUTTON2 (MB4 / MB5) to be bound. Other buttons are filtered out in the hook callback.
- **Linux/macOS tray + global-shortcut compat.** v1 is Windows-only.
- **First-run wizard step 6 (Bind PTT hotkey)** lives in Phase 3's first-run scaffolding. Phase 5 adds the UI components it will reuse; we do not modify the wizard itself.
- **Saving the previous window position when minimizing to tray** — the OS preserves it across hide/show.

---

## File structure after this plan

```
src-tauri/
├── Cargo.toml                              # MODIFIED: + tauri-plugin-global-shortcut, tauri-plugin-tray-icon, windows
├── capabilities/
│   └── default.json                        # MODIFIED: + global-shortcut + tray-icon permissions
├── tauri.conf.json                         # MODIFIED: + plugins.global-shortcut + plugins.tray-icon
├── icons/
│   ├── tray-idle.png                       # NEW: 32x32 gray dot
│   └── tray-capturing.png                  # NEW: 32x32 green dot
└── src/
    ├── hotkey/
    │   ├── mod.rs                          # NEW: HotkeyManager + Binding + HotkeyEvent
    │   ├── binding.rs                      # NEW: Binding type + parse/display (TDD)
    │   ├── keyboard.rs                     # NEW: KeyboardBackend wrapping tauri-plugin-global-shortcut
    │   └── mouse.rs                        # NEW: MouseBackend with WH_MOUSE_LL on a dedicated thread
    ├── capture/
    │   ├── mode.rs                         # NEW: CaptureMode enum + Arc<RwLock<…>> handle (TDD)
    │   └── controller.rs                   # MODIFIED: + HotkeyEvent arm in select!; PTT gating
    ├── tray/
    │   ├── mod.rs                          # NEW: TrayHandle + builder
    │   └── menu.rs                         # NEW: menu items + i18n key lookup
    ├── commands/
    │   ├── mod.rs                          # MODIFIED: pub mod hotkey
    │   ├── hotkey.rs                       # NEW: hotkey_get_binding / hotkey_set_binding / hotkey_capture_next / hotkey_clear
    │   ├── capture.rs                      # MODIFIED: + capture_set_mode / capture_get_mode commands
    │   └── settings.rs                     # MODIFIED: settings_set notices "capture_mode" + "hotkey_binding" and rewires
    └── lib.rs                              # MODIFIED: + plugins init; manage HotkeyManager, CaptureMode handle, Tray; window close-to-tray

src/
├── lib/
│   └── tauri.ts                            # MODIFIED: + hotkeyApi, captureModeApi, Binding type, listenHotkeyCapture
├── stores/
│   ├── captureStore.ts                     # MODIFIED: + captureMode, setCaptureMode, hotkeyBinding, setHotkeyBinding
│   └── settingsStore.ts                    # MODIFIED: load also reads capture_mode + hotkey_binding
├── components/
│   ├── SettingsDrawer.tsx                  # MODIFIED: + <CaptureSettings/> section above <VocabSettings/>
│   ├── CaptureSettings.tsx                 # NEW: mode dropdown + hotkey row + HotkeyBinder
│   └── HotkeyBinder.tsx                    # NEW: "Vincular" button → capture overlay
├── i18n/locales/
│   ├── pt-BR.json                          # MODIFIED: + settings.captureMode*, settings.hotkey*, tray.*
│   └── en.json                             # MODIFIED: same
├── main.tsx                                # MODIFIED: + plugin init lines (if the JS plugin requires one)
└── __tests__/
    ├── CaptureSettings.test.tsx            # NEW
    ├── HotkeyBinder.test.tsx               # NEW: Esc cancels; first event wins
    └── settingsStore.test.ts               # MODIFIED: capture_mode round-trip
```

Each module has one responsibility: `hotkey::keyboard` / `hotkey::mouse` are the only Win32-touching files; `capture::mode` is the single source of truth for the current mode; `tray::menu` knows i18n; `capture::controller` only consumes already-decoded events.

---

# Phase 5 tasks

## Task 1: Add Phase 5 dependencies + tauri.conf.json plugin entries

We add three Rust crates and two Tauri plugins. The frontend pulls in one new package (`@tauri-apps/plugin-global-shortcut`) so the JS side can call `register`/`unregister` if we ever need to; for v1 only the Rust side touches the plugin, but the JS import is required for the plugin runtime to attach on startup.

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `package.json`

- [ ] **Step 1: Add Rust dependencies to `src-tauri/Cargo.toml`**

Inside `[dependencies]`, alphabetical position:

```toml
tauri-plugin-global-shortcut = "2"
tauri-plugin-tray-icon = "2"   # if absent on crates.io by the time this runs, delete this line and use tauri's built-in `tray::TrayIconBuilder` instead — the rest of the plan is unchanged.
windows = { version = "0.58", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_System_LibraryLoader",
    "Win32_Foundation",
] }
```

- [ ] **Step 2: Register the plugins in `src-tauri/tauri.conf.json`**

Add a top-level `plugins` object (sibling of `app`, `bundle`):

```jsonc
"plugins": {
  "global-shortcut": {
    "all": true
  },
  "tray-icon": {}
}
```

(If `tauri-plugin-tray-icon` is not on crates.io, drop the `tray-icon` key from `plugins` and skip Step 4 of this task — the built-in `tauri::tray` does not require a config entry.)

- [ ] **Step 3: Add the capability permissions in `src-tauri/capabilities/default.json`**

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capabilities for the main window. The audio asset:// scope is declared in tauri.conf.json under app.security.assetProtocol.",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "global-shortcut:default",
    "tray-icon:default"
  ]
}
```

(Drop `tray-icon:default` if you skipped the plugin in Step 2.)

- [ ] **Step 4: Add the JS plugin package to `package.json`**

Under `dependencies`:

```json
"@tauri-apps/plugin-global-shortcut": "^2.0.0"
```

Run:

```powershell
npm install
```

Expected: lock file updates; no audit errors.

- [ ] **Step 5: Confirm the workspace still compiles**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

Expected: clean build. No warnings from the new deps (the `windows` crate is feature-gated to the four bits we use). If `cargo build` complains about a Tauri capability identifier that does not exist (plugin name drift), update the identifier in `capabilities/default.json` to match what the plugin's docs print.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/capabilities/default.json package.json package-lock.json
git commit -m "feat(deps): add global-shortcut + tray + windows-rs for phase 5"
```

---

## Task 2: `Binding` type with parse + display (TDD)

The `Binding` is the canonical representation of "what fires PTT". It is two things:
- A `kind` discriminant (`Key` or `Mouse`).
- A `code` string. For keyboard, the spec's plugin uses `KeyCode` / `Code` strings (e.g. `"ControlRight"`, `"Space"`, `"F13"`). For mouse, we use four fixed labels: `"MouseButton4"` (XBUTTON1), `"MouseButton5"` (XBUTTON2). Left/Right/Middle are deliberately not allowed.

A user-facing `Display` impl renders human-readable labels (`"Right Ctrl"`, `"Mouse Button 5"`). i18n happens in the frontend; the Rust side keeps stable codes for persistence.

**Files:**
- Create: `src-tauri/src/hotkey/mod.rs`
- Create: `src-tauri/src/hotkey/binding.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod hotkey;`)

- [ ] **Step 1: Create `src-tauri/src/hotkey/mod.rs`**

```rust
pub mod binding;
pub mod keyboard;
pub mod mouse;

pub use binding::{Binding, BindingError, BindingKind};

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use parking_lot::RwLock;

#[derive(Debug, Clone, Copy)]
pub enum HotkeyEvent {
    Press,
    Release,
}

/// `Send + Sync` handle held in Tauri state. Composes both backends. Only
/// the binding's `kind` decides which backend is armed at any moment.
#[derive(Clone)]
pub struct HotkeyManager {
    inner: Arc<HotkeyManagerInner>,
}

struct HotkeyManagerInner {
    binding: RwLock<Option<Binding>>,
    event_tx: Sender<HotkeyEvent>,
    event_rx: Receiver<HotkeyEvent>,
    keyboard: keyboard::KeyboardBackend,
    mouse: mouse::MouseBackend,
}

impl HotkeyManager {
    pub fn new(app: tauri::AppHandle) -> anyhow::Result<Self> {
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let keyboard = keyboard::KeyboardBackend::new(app, event_tx.clone())?;
        let mouse = mouse::MouseBackend::new(event_tx.clone())?;
        Ok(Self {
            inner: Arc::new(HotkeyManagerInner {
                binding: RwLock::new(None),
                event_tx,
                event_rx,
                keyboard,
                mouse,
            }),
        })
    }

    pub fn subscribe(&self) -> Receiver<HotkeyEvent> {
        self.inner.event_rx.clone()
    }

    pub fn current_binding(&self) -> Option<Binding> {
        self.inner.binding.read().clone()
    }

    pub fn set_binding(&self, b: Binding) -> anyhow::Result<()> {
        // Unregister both backends first so a re-set with the same code does
        // not double-fire.
        self.inner.keyboard.unwatch();
        self.inner.mouse.unwatch();
        match b.kind {
            BindingKind::Key => self.inner.keyboard.watch(&b.code)?,
            BindingKind::Mouse => self.inner.mouse.watch(&b.code)?,
        }
        *self.inner.binding.write() = Some(b);
        Ok(())
    }

    pub fn clear_binding(&self) {
        self.inner.keyboard.unwatch();
        self.inner.mouse.unwatch();
        *self.inner.binding.write() = None;
    }

    /// Arm both backends in capture mode. Returns the first press received,
    /// or an error if `timeout` elapses or capture is cancelled by Esc.
    pub fn capture_next_press(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Binding, BindingError> {
        // The keyboard backend's capture sends a sentinel for Esc; the mouse
        // backend forwards any allowed XBUTTON. Whichever channel fires first
        // wins. Implementation lives in keyboard.rs / mouse.rs; both write
        // into a oneshot channel created here.
        let (tx, rx) = crossbeam_channel::bounded::<Result<Binding, BindingError>>(1);
        self.inner.keyboard.start_capture(tx.clone())?;
        self.inner.mouse.start_capture(tx)?;
        let result = rx.recv_timeout(timeout).unwrap_or_else(|_| {
            Err(BindingError::Timeout)
        });
        self.inner.keyboard.stop_capture();
        self.inner.mouse.stop_capture();
        // Re-arm the existing binding if one was set before capture began.
        if let Some(b) = self.current_binding() {
            let _ = self.set_binding(b);
        }
        result
    }
}
```

- [ ] **Step 2: Create `src-tauri/src/hotkey/binding.rs`**

```rust
//! `Binding` — what physical key or mouse button fires PTT. Persisted as JSON
//! under `settings.hotkey_binding`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingKind {
    Key,
    Mouse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub kind: BindingKind,
    /// Backend-stable code. For `Key`: a `tauri-plugin-global-shortcut`
    /// `Code` token (`"ControlRight"`, `"Space"`, `"F13"`). For `Mouse`:
    /// exactly one of `"MouseButton4"`, `"MouseButton5"`.
    pub code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BindingError {
    #[error("capture cancelled")]
    Cancelled,
    #[error("capture timed out")]
    Timeout,
    #[error("unsupported mouse button: {0}")]
    UnsupportedMouseButton(String),
    #[error("invalid key code: {0}")]
    InvalidKeyCode(String),
}

impl Binding {
    pub fn key(code: impl Into<String>) -> Self {
        Self {
            kind: BindingKind::Key,
            code: code.into(),
        }
    }

    pub fn mouse(code: impl Into<String>) -> Self {
        Self {
            kind: BindingKind::Mouse,
            code: code.into(),
        }
    }

    /// Human-readable label. The frontend can override per-locale; this is
    /// the fallback English label.
    pub fn label(&self) -> String {
        match self.kind {
            BindingKind::Mouse => match self.code.as_str() {
                "MouseButton4" => "Mouse Button 4".to_string(),
                "MouseButton5" => "Mouse Button 5".to_string(),
                other => other.to_string(),
            },
            BindingKind::Key => prettify_key(&self.code),
        }
    }
}

fn prettify_key(code: &str) -> String {
    // A tiny lookup for the most-bound PTT keys. Everything else falls
    // through to the raw code so we never lose information.
    match code {
        "ControlLeft" => "Left Ctrl".into(),
        "ControlRight" => "Right Ctrl".into(),
        "ShiftLeft" => "Left Shift".into(),
        "ShiftRight" => "Right Shift".into(),
        "AltLeft" => "Left Alt".into(),
        "AltRight" => "Right Alt".into(),
        "Space" => "Space".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_binding_round_trips_json() {
        let b = Binding::key("ControlRight");
        let s = serde_json::to_string(&b).unwrap();
        let parsed: Binding = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, b);
    }

    #[test]
    fn mouse_binding_round_trips_json() {
        let b = Binding::mouse("MouseButton5");
        let s = serde_json::to_string(&b).unwrap();
        let parsed: Binding = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, b);
    }

    #[test]
    fn json_uses_lowercase_kind_tag() {
        let b = Binding::key("Space");
        let s = serde_json::to_string(&b).unwrap();
        assert!(s.contains("\"kind\":\"key\""), "got {s}");
    }

    #[test]
    fn label_renders_known_keys() {
        assert_eq!(Binding::key("ControlRight").label(), "Right Ctrl");
        assert_eq!(Binding::key("Space").label(), "Space");
        // Unknown keys pass through.
        assert_eq!(Binding::key("F13").label(), "F13");
    }

    #[test]
    fn label_renders_mouse_buttons() {
        assert_eq!(Binding::mouse("MouseButton4").label(), "Mouse Button 4");
        assert_eq!(Binding::mouse("MouseButton5").label(), "Mouse Button 5");
    }
}
```

- [ ] **Step 3: Declare `hotkey` in `src-tauri/src/lib.rs`**

Insert `pub mod hotkey;` in alphabetical order:

```rust
pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod hallucination;
pub mod hotkey;
pub mod logging;
pub mod paths;
pub mod routing;
pub mod stt;
pub mod tray;        // added in Task 9
pub mod utterance;
pub mod vad;
pub mod vocab;
```

(`tray` is added in Task 9; leave the line in now so the alphabetical list is stable.)

- [ ] **Step 4: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml hotkey::binding
```

Expected: 5 passed. (The `keyboard.rs` / `mouse.rs` modules don't exist yet — `cargo build` will fail at the `pub mod` lines in `hotkey/mod.rs`. **Stub them out** with one-line empty modules until Tasks 3 and 4 land them; otherwise comment out the `pub mod keyboard; pub mod mouse;` lines and the `keyboard::KeyboardBackend` / `mouse::MouseBackend` references in `HotkeyManagerInner`. Easier: stub `pub struct KeyboardBackend; impl KeyboardBackend { pub fn new(_a: tauri::AppHandle, _t: crossbeam_channel::Sender<HotkeyEvent>) -> anyhow::Result<Self> { Ok(Self) } pub fn watch(&self, _c: &str) -> anyhow::Result<()> { Ok(()) } pub fn unwatch(&self) {} pub fn start_capture(&self, _t: crossbeam_channel::Sender<Result<Binding, BindingError>>) -> anyhow::Result<()> { Ok(()) } pub fn stop_capture(&self) {} }` — same for `MouseBackend`. Tasks 3 and 4 replace them.)

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/hotkey src-tauri/src/lib.rs
git commit -m "feat(hotkey): Binding type + HotkeyManager skeleton"
```

---

## Task 3: `KeyboardBackend` via `tauri-plugin-global-shortcut`

The plugin exposes `Shortcut::with_handler(|app, shortcut, event| …)`. In v2, `event.state()` returns `ShortcutState::Pressed` on key-down and `ShortcutState::Released` on key-up. We register/unregister at `watch`/`unwatch` time. In capture mode we register a wide net (a "raw event listener" hook) — see the plugin's `on_shortcut` API for the device-agnostic listener.

If a sufficiently granular "any key" listener is not exposed by the plugin (older v2 versions only deliver registered shortcuts), the fallback is: capture mode in the **keyboard** backend only listens for `Escape` (so the user can always cancel from the keyboard), and the mouse backend handles real first-press detection. Document this fallback in the file.

**Files:**
- Modify: `src-tauri/src/hotkey/keyboard.rs` (replace stub)
- Modify: `src-tauri/src/lib.rs` (`tauri::Builder::default().plugin(tauri_plugin_global_shortcut::Builder::new().build())`)

- [ ] **Step 1: Replace `src-tauri/src/hotkey/keyboard.rs`**

```rust
//! Keyboard hotkey backend wrapping `tauri-plugin-global-shortcut`.
//!
//! Two modes:
//! - **Watch** — a single `Shortcut` is registered; press/release events are
//!   forwarded to the manager's `event_tx`.
//! - **Capture** — listen for ANY key press (via the plugin's raw-listener
//!   API if available; else just listen for `Escape` so Esc-cancel still
//!   works). The first event becomes the new binding.

use std::sync::Arc;

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

use super::{Binding, BindingError, HotkeyEvent};

pub struct KeyboardBackend {
    app: AppHandle,
    event_tx: Sender<HotkeyEvent>,
    current: Arc<Mutex<Option<Shortcut>>>,
    capture_tx: Arc<Mutex<Option<Sender<Result<Binding, BindingError>>>>>,
}

impl KeyboardBackend {
    pub fn new(app: AppHandle, event_tx: Sender<HotkeyEvent>) -> anyhow::Result<Self> {
        Ok(Self {
            app,
            event_tx,
            current: Arc::new(Mutex::new(None)),
            capture_tx: Arc::new(Mutex::new(None)),
        })
    }

    pub fn watch(&self, code: &str) -> anyhow::Result<()> {
        self.unwatch();
        let shortcut = parse_code(code)?;
        let event_tx = self.event_tx.clone();
        let plugin = self.app.global_shortcut();
        plugin.on_shortcut(shortcut.clone(), move |_app, _shortcut, event| {
            match event.state() {
                ShortcutState::Pressed => {
                    let _ = event_tx.send(HotkeyEvent::Press);
                }
                ShortcutState::Released => {
                    let _ = event_tx.send(HotkeyEvent::Release);
                }
            }
        })?;
        *self.current.lock() = Some(shortcut);
        Ok(())
    }

    pub fn unwatch(&self) {
        if let Some(shortcut) = self.current.lock().take() {
            let _ = self.app.global_shortcut().unregister(shortcut);
        }
    }

    pub fn start_capture(
        &self,
        tx: Sender<Result<Binding, BindingError>>,
    ) -> anyhow::Result<()> {
        // Always at least catch Escape so the user can cancel from the
        // keyboard. If the plugin version supports raw any-key, also wire
        // that. For v1 we only register `Escape` and a small set of common
        // PTT keys (left/right ctrl, shift, alt, space, F13-F19); the user
        // can also bind a mouse button to capture from the mouse backend.
        let common_codes = &[
            Code::Escape,
            Code::ControlLeft, Code::ControlRight,
            Code::ShiftLeft, Code::ShiftRight,
            Code::AltLeft, Code::AltRight,
            Code::Space,
            Code::F13, Code::F14, Code::F15, Code::F16, Code::F17, Code::F18, Code::F19,
        ];
        *self.capture_tx.lock() = Some(tx.clone());
        let capture_tx = self.capture_tx.clone();
        let plugin = self.app.global_shortcut();
        for code in common_codes {
            let shortcut = Shortcut::new(None, *code);
            let tx_for_handler = tx.clone();
            let capture_tx_for_handler = capture_tx.clone();
            plugin.on_shortcut(shortcut, move |_app, shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                // Only fire if capture is still armed; the manager clears
                // `capture_tx` after the first send.
                let still_armed = capture_tx_for_handler.lock().take().is_some();
                if !still_armed {
                    return;
                }
                let code_str = format!("{:?}", shortcut.key);
                if shortcut.key == Code::Escape {
                    let _ = tx_for_handler.send(Err(BindingError::Cancelled));
                } else {
                    let _ = tx_for_handler.send(Ok(Binding::key(code_str)));
                }
            })?;
        }
        Ok(())
    }

    pub fn stop_capture(&self) {
        *self.capture_tx.lock() = None;
        let plugin = self.app.global_shortcut();
        let _ = plugin.unregister_all();
        // Re-arm the current binding (caller `HotkeyManager::capture_next_press`
        // also does this; we belt-and-suspenders so double-stop is safe).
        if let Some(shortcut) = self.current.lock().clone() {
            let event_tx = self.event_tx.clone();
            let _ = plugin.on_shortcut(shortcut, move |_app, _s, event| {
                match event.state() {
                    ShortcutState::Pressed => { let _ = event_tx.send(HotkeyEvent::Press); }
                    ShortcutState::Released => { let _ = event_tx.send(HotkeyEvent::Release); }
                }
            });
        }
    }
}

fn parse_code(code: &str) -> Result<Shortcut, anyhow::Error> {
    // The plugin exposes Code as an enum; the simplest robust path is the
    // `Code::from_str` impl from `keyboard-types`. We accept the canonical
    // KeyboardEvent.code strings (e.g. "ControlRight").
    let parsed: Code = code.parse().map_err(|e| {
        anyhow::anyhow!("invalid key code {code:?}: {e}")
    })?;
    Ok(Shortcut::new(None, parsed))
}
```

> **Note on plugin API drift.** Tauri 2 plugin APIs evolve quickly. If by the time this runs `on_shortcut` no longer exists or has a different signature (e.g. `register(shortcut)` + `app.listen("plugin:global-shortcut:hotkey")`), adapt the wiring but **preserve the channel contract**: every press sends `HotkeyEvent::Press`; every release sends `HotkeyEvent::Release`. Nothing else changes.

- [ ] **Step 2: Initialize the plugin in `src-tauri/src/lib.rs`**

In `tauri::Builder::default()`, before `.manage(db.clone())`:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .manage(db.clone())
    // ...
```

- [ ] **Step 3: `cargo build` to confirm the API matches**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

If `Code::from_str` is missing, add `use std::str::FromStr;` and call `Code::from_str(code)`. If the plugin requires `Modifiers::empty()` rather than `None` for the first argument of `Shortcut::new`, adapt. Test errors here are normal — fix as `cargo` reports them.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/hotkey/keyboard.rs src-tauri/src/lib.rs
git commit -m "feat(hotkey): keyboard backend via global-shortcut plugin"
```

---

## Task 4: `MouseBackend` via `WH_MOUSE_LL` on a dedicated thread

This is the load-bearing Windows-specific piece. We install a low-level mouse hook, pump messages, and forward XBUTTON1 / XBUTTON2 events. Three invariants:

1. The hook callback **never modifies** the `MSLLHOOKSTRUCT` and **always returns** `CallNextHookEx(...)`. The hook is purely observational. Antivirus heuristics are happier if the hook is short and non-blocking; we only do a `crossbeam_channel::Sender::send` inside, which is non-blocking.
2. `SetWindowsHookExW` for a low-level hook does not need a DLL, but the hook callback runs **on the thread that installed the hook**, and that thread MUST have a message pump (`GetMessageW` / `TranslateMessage` / `DispatchMessageW`). So we own a dedicated thread for the lifetime of the backend.
3. State shared between the hook callback and the manager is a `&'static` channel sender. We can't capture environment in a `unsafe extern "system"` fn, so we use a `OnceLock<Sender<MouseEvent>>` for the live channel.

**Files:**
- Modify: `src-tauri/src/hotkey/mouse.rs`

- [ ] **Step 1: Replace `src-tauri/src/hotkey/mouse.rs`**

```rust
//! Mouse-button hotkey backend.
//!
//! Owns a dedicated OS thread that installs `WH_MOUSE_LL`, pumps the message
//! loop, and forwards `XBUTTONDOWN` / `XBUTTONUP` for XBUTTON1 (MB4) and
//! XBUTTON2 (MB5). The hook is read-only; we always call `CallNextHookEx`.
//!
//! Other mouse buttons (left/right/middle, wheel) are filtered out — binding
//! the left mouse button would brick the UI, and the v1 spec only requires
//! MB4/MB5 anyway.

use std::sync::OnceLock;

use crossbeam_channel::Sender;
use parking_lot::Mutex;

use super::{Binding, BindingError, HotkeyEvent};

/// What the hook callback emits. Press/release for whichever XBUTTON.
#[derive(Debug, Clone, Copy)]
enum MouseHookEvent {
    XButtonDown(u16), // 1 or 2
    XButtonUp(u16),
}

/// Global channel used by the hook callback. `OnceLock` is sufficient because
/// the hook thread is a singleton for the process lifetime.
static HOOK_TX: OnceLock<Sender<MouseHookEvent>> = OnceLock::new();

pub struct MouseBackend {
    event_tx: Sender<HotkeyEvent>,
    bound_button: std::sync::Arc<Mutex<Option<u16>>>,
    capture_tx: std::sync::Arc<Mutex<Option<Sender<Result<Binding, BindingError>>>>>,
}

impl MouseBackend {
    pub fn new(event_tx: Sender<HotkeyEvent>) -> anyhow::Result<Self> {
        // Spawn the singleton hook thread the first time we're constructed.
        let (tx, rx) = crossbeam_channel::unbounded();
        HOOK_TX
            .set(tx)
            .map_err(|_| anyhow::anyhow!("MouseBackend can only be constructed once"))?;
        std::thread::Builder::new()
            .name("voicetabs-mouse-hook".into())
            .spawn(install_and_pump)
            .map_err(|e| anyhow::anyhow!("spawn mouse hook thread: {e}"))?;

        let bound_button = std::sync::Arc::new(Mutex::new(None::<u16>));
        let capture_tx = std::sync::Arc::new(Mutex::new(None::<Sender<Result<Binding, BindingError>>>));

        // Spawn a forwarder that consumes raw MouseHookEvent and dispatches:
        // - if a binding matches the button: emit HotkeyEvent::Press/Release
        // - if capture is armed: emit Binding into capture_tx
        {
            let event_tx = event_tx.clone();
            let bound_button = bound_button.clone();
            let capture_tx = capture_tx.clone();
            std::thread::Builder::new()
                .name("voicetabs-mouse-dispatch".into())
                .spawn(move || {
                    while let Ok(evt) = rx.recv() {
                        // Capture mode wins over watch mode: a fresh
                        // start_capture should not also fire HotkeyEvent.
                        let captured = match evt {
                            MouseHookEvent::XButtonDown(n) => {
                                if let Some(sender) = capture_tx.lock().take() {
                                    let code = format!("MouseButton{}", n + 3);
                                    let _ = sender.send(Ok(Binding::mouse(code)));
                                    true
                                } else {
                                    false
                                }
                            }
                            MouseHookEvent::XButtonUp(_) => false,
                        };
                        if captured {
                            continue;
                        }
                        let matches = bound_button.lock().map(|b| match evt {
                            MouseHookEvent::XButtonDown(n) => b == n,
                            MouseHookEvent::XButtonUp(n) => b == n,
                        });
                        if !matches.unwrap_or(false) {
                            continue;
                        }
                        match evt {
                            MouseHookEvent::XButtonDown(_) => { let _ = event_tx.send(HotkeyEvent::Press); }
                            MouseHookEvent::XButtonUp(_) => { let _ = event_tx.send(HotkeyEvent::Release); }
                        }
                    }
                })
                .map_err(|e| anyhow::anyhow!("spawn mouse dispatch thread: {e}"))?;
        }

        Ok(Self { event_tx, bound_button, capture_tx })
    }

    pub fn watch(&self, code: &str) -> anyhow::Result<()> {
        let n = code_to_xbutton(code)?;
        *self.bound_button.lock() = Some(n);
        Ok(())
    }

    pub fn unwatch(&self) {
        *self.bound_button.lock() = None;
    }

    pub fn start_capture(
        &self,
        tx: Sender<Result<Binding, BindingError>>,
    ) -> anyhow::Result<()> {
        *self.capture_tx.lock() = Some(tx);
        Ok(())
    }

    pub fn stop_capture(&self) {
        *self.capture_tx.lock() = None;
    }
}

fn code_to_xbutton(code: &str) -> Result<u16, BindingError> {
    match code {
        "MouseButton4" => Ok(1),
        "MouseButton5" => Ok(2),
        other => Err(BindingError::UnsupportedMouseButton(other.into())),
    }
}

// --- Win32 hook thread + callback --------------------------------------------

#[cfg(target_os = "windows")]
fn install_and_pump() {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        HHOOK, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_XBUTTONDOWN, WM_XBUTTONUP,
    };

    static HOOK_HANDLE: OnceLock<HHOOK> = OnceLock::new();

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let wm = wparam.0 as u32;
            if wm == WM_XBUTTONDOWN || wm == WM_XBUTTONUP {
                // High word of mouseData identifies XBUTTON1 (=1) or XBUTTON2 (=2).
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                let xbutton = ((info.mouseData >> 16) & 0xFFFF) as u16;
                if xbutton == 1 || xbutton == 2 {
                    if let Some(tx) = HOOK_TX.get() {
                        let evt = if wm == WM_XBUTTONDOWN {
                            MouseHookEvent::XButtonDown(xbutton)
                        } else {
                            MouseHookEvent::XButtonUp(xbutton)
                        };
                        let _ = tx.send(evt);
                    }
                }
            }
        }
        // ALWAYS chain. We never consume mouse input.
        CallNextHookEx(*HOOK_HANDLE.get().unwrap_or(&HHOOK::default()), code, wparam, lparam)
    }

    unsafe {
        let hmod = GetModuleHandleW(None).expect("GetModuleHandleW");
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(proc), hmod, 0)
            .expect("SetWindowsHookExW(WH_MOUSE_LL)");
        let _ = HOOK_HANDLE.set(hook);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn install_and_pump() {
    // Non-Windows is out of scope for v1; the thread just parks.
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_to_xbutton_accepts_mb4_mb5() {
        assert_eq!(code_to_xbutton("MouseButton4").unwrap(), 1);
        assert_eq!(code_to_xbutton("MouseButton5").unwrap(), 2);
    }

    #[test]
    fn code_to_xbutton_rejects_left_button() {
        assert!(matches!(
            code_to_xbutton("MouseButtonLeft"),
            Err(BindingError::UnsupportedMouseButton(_))
        ));
    }
}
```

> **Self-review checkpoint (hook safety):**
> - The callback **never** returns anything other than `CallNextHookEx(...)`. Search for the word `return` in the callback to confirm only one tail-position call to `CallNextHookEx` exists.
> - The callback **never** writes through the `*MSLLHOOKSTRUCT`. Search for `*mut`; you should find none. The cast is `*const`.
> - The hook lives on its own thread (`voicetabs-mouse-hook`). The forwarder is a second thread (`voicetabs-mouse-dispatch`). The main process never owns the hook directly.
> - The `OnceLock<Sender>` ensures the hook can send without a runtime borrow check.
> - The forwarder **drops** events that don't match the bound button OR an armed capture, so we never accidentally start a PTT utterance on every mouse click.

- [ ] **Step 2: Run the tests + build**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml hotkey::mouse
cargo build --manifest-path src-tauri\Cargo.toml
```

Expected: 2 unit tests pass; clean build (linker pulls in the four `windows` features we declared).

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/hotkey/mouse.rs
git commit -m "feat(hotkey): mouse backend with WH_MOUSE_LL hook (read-only)"
```

---

## Task 5: `CaptureMode` enum + `Arc<RwLock<…>>` handle (TDD)

The mode lives in one place: `Arc<RwLock<CaptureMode>>`, managed by Tauri, read by the controller worker on every relevant event.

**Files:**
- Create: `src-tauri/src/capture/mode.rs`
- Modify: `src-tauri/src/capture/mod.rs`

- [ ] **Step 1: Inspect `src-tauri/src/capture/mod.rs` and add `pub mod mode;` + re-export**

```rust
pub mod controller;
pub mod mode;

pub use controller::{CaptureController, CaptureStatus, UtteranceFinalized};
pub use mode::{CaptureMode, CaptureModeHandle};
```

- [ ] **Step 2: Create `src-tauri/src/capture/mode.rs`**

```rust
//! Capture mode — `AlwaysOn` (VAD edges drive boundaries) or `Ptt` (hotkey
//! drives boundaries).
//!
//! The handle is `Send + Sync + Clone`; the controller worker reads it on
//! every event. Writes happen from the `settings_set` command when the user
//! changes the dropdown.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    AlwaysOn,
    Ptt,
}

impl Default for CaptureMode {
    fn default() -> Self {
        CaptureMode::AlwaysOn
    }
}

impl CaptureMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "always_on" => Some(CaptureMode::AlwaysOn),
            "ptt" => Some(CaptureMode::Ptt),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CaptureMode::AlwaysOn => "always_on",
            CaptureMode::Ptt => "ptt",
        }
    }
}

#[derive(Clone, Default)]
pub struct CaptureModeHandle {
    inner: Arc<RwLock<CaptureMode>>,
}

impl CaptureModeHandle {
    pub fn new(initial: CaptureMode) -> Self {
        Self {
            inner: Arc::new(RwLock::new(initial)),
        }
    }

    pub fn get(&self) -> CaptureMode {
        *self.inner.read()
    }

    pub fn set(&self, mode: CaptureMode) {
        *self.inner.write() = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trip() {
        for m in [CaptureMode::AlwaysOn, CaptureMode::Ptt] {
            assert_eq!(CaptureMode::parse(m.as_str()), Some(m));
        }
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert!(CaptureMode::parse("toggle").is_none());
    }

    #[test]
    fn json_uses_snake_case() {
        let s = serde_json::to_string(&CaptureMode::AlwaysOn).unwrap();
        assert_eq!(s, "\"always_on\"");
        let s = serde_json::to_string(&CaptureMode::Ptt).unwrap();
        assert_eq!(s, "\"ptt\"");
    }

    #[test]
    fn handle_get_set_are_visible_across_clones() {
        let a = CaptureModeHandle::new(CaptureMode::AlwaysOn);
        let b = a.clone();
        b.set(CaptureMode::Ptt);
        assert_eq!(a.get(), CaptureMode::Ptt);
    }

    #[test]
    fn default_is_always_on() {
        assert_eq!(CaptureMode::default(), CaptureMode::AlwaysOn);
        assert_eq!(CaptureModeHandle::default().get(), CaptureMode::AlwaysOn);
    }
}
```

- [ ] **Step 3: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml capture::mode
```

Expected: 5 passed.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/capture
git commit -m "feat(capture): CaptureMode enum + Arc<RwLock<…>> handle"
```

---

## Task 6: Capture controller — hotkey gating in `select!` (load-bearing)

We modify `CaptureController::spawn` to accept the `CaptureModeHandle` and the `HotkeyManager`'s receiver, and we add a third arm to the `select!` block in the worker loop. The semantics:

- In `AlwaysOn`: hotkey events arrive but are **dropped on the floor**. VAD edges drive the utterance lifecycle exactly as in Phase 4.
- In `Ptt`: VAD `RisingEdge` / `FallingEdge` events are observed (the model still runs because the hallucination filter uses RMS over the same buffer) but **not allowed to start or finalize an utterance**. Instead:
  - `HotkeyEvent::Press` → snapshot `(start_tab_id, vocab, language)`, force the VAD state machine into `Speaking`, and synthetically inject a `RisingEdge` into the `UtteranceBuilder` so the pre-roll lands correctly.
  - `HotkeyEvent::Release` → force VAD to `Idle`, synthetically inject a `FallingEdge` so the builder finalizes, publish the utterance.
- A spurious `Press` while `current_meta.is_some()` is treated as a no-op (probably hotkey auto-repeat or key chatter). A spurious `Release` with `current_meta.is_none()` is also a no-op.

**Files:**
- Modify: `src-tauri/src/capture/controller.rs`
- Modify: `src-tauri/src/lib.rs` (pass the new args at `CaptureController::spawn` time)

- [ ] **Step 1: Update `CaptureController::spawn` signature in `controller.rs`**

```rust
use crate::capture::mode::CaptureModeHandle;
use crate::hotkey::HotkeyEvent;

pub struct CaptureController {
    cmd_tx: Sender<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    utterance_rx: Receiver<UtteranceFinalized>,
}

impl CaptureController {
    pub fn spawn(
        output_dir: PathBuf,
        db: Db,
        active_tab: ActiveTab,
        mode: CaptureModeHandle,
        hotkey_rx: Receiver<HotkeyEvent>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let (utt_tx, utt_rx) = unbounded::<UtteranceFinalized>();
        let status = Arc::new(Mutex::new(CaptureStatus::Idle));
        let status_for_worker = status.clone();
        std::thread::Builder::new()
            .name("voicetabs-capture".into())
            .spawn(move || {
                worker_loop(
                    cmd_rx,
                    status_for_worker,
                    output_dir,
                    db,
                    active_tab,
                    mode,
                    hotkey_rx,
                    utt_tx,
                )
            })
            .expect("spawn capture thread");
        Self { cmd_tx, status, utterance_rx: utt_rx }
    }
    // start / stop / status / utterance_receiver unchanged
}
```

- [ ] **Step 2: Add the new arm to the `select!` in `worker_loop`**

```rust
#[allow(clippy::too_many_arguments)]
fn worker_loop(
    cmd_rx: Receiver<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    output_dir: PathBuf,
    db: Db,
    active_tab: ActiveTab,
    mode: CaptureModeHandle,
    hotkey_rx: Receiver<HotkeyEvent>,
    utt_tx: Sender<UtteranceFinalized>,
) {
    // ...existing locals (stream_handle, vad_model, vad_sm, builder, accumulator,
    //   current_meta)...

    loop {
        let frames_rx = stream_handle.as_ref().map(|h| h.frames.clone());

        if let Some(frames_rx) = frames_rx {
            select! {
                recv(cmd_rx) -> cmd => { /* unchanged */ }
                recv(frames_rx) -> frame => {
                    // unchanged through `accumulator.extend_from_slice(...)`
                    let model = vad_model.as_mut().expect("model loaded above");
                    accumulator.extend_from_slice(&frame_16k);
                    process_chunks(
                        &mut accumulator,
                        model,
                        &mut vad_sm,
                        &mut builder,
                        &mut current_meta,
                        &db,
                        &active_tab,
                        &mode,
                        &utt_tx,
                    );
                }
                recv(hotkey_rx) -> evt => {
                    let Ok(evt) = evt else { continue };
                    if mode.get() != CaptureMode::Ptt {
                        // AlwaysOn: hotkey is informational only; drop.
                        continue;
                    }
                    let ts_ms = unix_now_ms();
                    match evt {
                        HotkeyEvent::Press => {
                            if current_meta.is_some() { continue; } // already in utterance
                            let Some(tab_id) = active_tab.snapshot() else {
                                tracing::warn!("PTT press with no active tab; ignoring");
                                continue;
                            };
                            let vocab_terms: Vec<String> =
                                settings_repo::get::<Vec<String>>(&db, "vocab_terms")
                                    .ok().flatten().unwrap_or_default();
                            let language: String =
                                settings_repo::get::<String>(&db, "language")
                                    .ok().flatten().unwrap_or_else(|| "pt".into());
                            current_meta = Some(UtteranceMeta {
                                start_tab_id: tab_id,
                                vocab_terms,
                                language,
                            });
                            // Force the VAD state machine into Speaking so a
                            // VAD-driven FallingEdge in PTT mode is also
                            // suppressed (we drop it in process_chunks).
                            vad_sm.force_speaking();
                            // Synthesize a rising edge into the builder so its
                            // pre-roll bookkeeping runs.
                            let _ = builder.on_vad_event(VadEvent::RisingEdge { timestamp_ms: ts_ms });
                        }
                        HotkeyEvent::Release => {
                            let Some(meta) = current_meta.take() else { continue };
                            if let Some(finalized) = builder.on_vad_event(VadEvent::FallingEdge { timestamp_ms: ts_ms }) {
                                publish_utterance(&utt_tx, finalized, meta);
                            }
                            vad_sm.force_idle();
                        }
                    }
                }
            }
        } else {
            // Idle: block on commands only (hotkey events buffer in the channel).
            // unchanged
        }
    }
}
```

> **Note:** `force_speaking()` is a new method on `VadStateMachine`. Add it in the same patch:
>
> ```rust
> // src-tauri/src/vad/state.rs
> pub fn force_speaking(&mut self) {
>     self.state = VadState::Speaking;
>     self.above_count = 0;
>     self.below_count = 0;
> }
> ```
>
> And add a unit test alongside the existing `force_idle` test.

- [ ] **Step 3: Update `process_chunks` to gate VAD edges by mode**

```rust
#[allow(clippy::too_many_arguments)]
fn process_chunks(
    accumulator: &mut Vec<f32>,
    model: &mut VadModel,
    sm: &mut VadStateMachine,
    builder: &mut UtteranceBuilder,
    current_meta: &mut Option<UtteranceMeta>,
    db: &Db,
    active_tab: &ActiveTab,
    mode: &CaptureModeHandle,
    utt_tx: &Sender<UtteranceFinalized>,
) {
    while accumulator.len() >= CHUNK_SAMPLES {
        let chunk: Vec<f32> = accumulator.drain(..CHUNK_SAMPLES).collect();
        let prob = match model.predict(&chunk) { Ok(p) => p, Err(_) => continue };
        let ts_ms = unix_now_ms();
        let event = sm.observe(prob, ts_ms);
        // ── PTT mode: feed the builder so pre-roll keeps moving and the
        //    builder's max-duration cap still applies, but VAD edges are
        //    NOT allowed to start or finalize an utterance.
        if mode.get() == CaptureMode::Ptt {
            let _ = builder.push_frame(&chunk);
            continue;
        }
        // ── AlwaysOn mode: existing Phase 4 logic.
        if let Some(event) = event {
            if matches!(event, VadEvent::RisingEdge { .. }) {
                // (unchanged: snapshot tab_id, vocab, language; populate current_meta)
            }
            let _ = builder.push_frame(&chunk);
            if let Some(finalized) = builder.on_vad_event(event) {
                if let Some(meta) = current_meta.take() {
                    publish_utterance(utt_tx, finalized, meta);
                }
            }
        } else if let Some(finalized) = builder.push_frame(&chunk) {
            if let Some(meta) = current_meta.take() {
                publish_utterance(utt_tx, finalized, meta);
            }
            sm.force_idle();
        }
    }
}
```

- [ ] **Step 4: Run the existing tests; nothing should regress**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml capture
cargo test --manifest-path src-tauri\Cargo.toml vad::state
```

Expected: all existing tests still pass. The new `force_speaking` test passes.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/capture/controller.rs src-tauri/src/vad/state.rs
git commit -m "feat(capture): PTT gating in worker; hotkey arm in select!"
```

---

## Task 7: Wire `HotkeyManager` + `CaptureModeHandle` into `lib.rs` setup()

The `CaptureController::spawn` call now takes two extra arguments. Both come from the `setup()` closure (after Tauri's `AppHandle` is available, since the `HotkeyManager` needs it).

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Move `CaptureController::spawn` call into `setup()`**

Because `HotkeyManager::new(app_handle)` needs the `AppHandle`, the controller can no longer be spawned before `tauri::Builder::default()`. Reorganize:

```rust
pub fn run() {
    // ...db/audio_dir/active_tab setup unchanged...

    let backend = "cpu".to_string();
    let stt_status = SttStatusHandle::new(SttStatus::Loading { backend: backend.clone() });
    let stt_status_for_state = stt_status.clone();

    let mode_handle = capture::CaptureModeHandle::new(
        load_capture_mode(&db).unwrap_or(capture::CaptureMode::AlwaysOn),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(db.clone())
        .manage(active_tab.clone())
        .manage(stt_status_for_state)
        .manage(mode_handle.clone())
        .invoke_handler(tauri::generate_handler![
            // existing handlers...
            commands::capture::capture_get_mode,
            commands::capture::capture_set_mode,
            commands::hotkey::hotkey_get_binding,
            commands::hotkey::hotkey_set_binding,
            commands::hotkey::hotkey_clear_binding,
            commands::hotkey::hotkey_capture_next,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            let hotkey_mgr = hotkey::HotkeyManager::new(app_handle.clone())
                .expect("init hotkey manager");
            // Restore the persisted binding if any.
            if let Ok(Some(json)) = db::settings::get::<serde_json::Value>(&db, "hotkey_binding") {
                if let Ok(b) = serde_json::from_value::<hotkey::Binding>(json) {
                    if let Err(e) = hotkey_mgr.set_binding(b) {
                        tracing::warn!("failed to register persisted hotkey binding: {e}");
                    }
                }
            }
            app.manage(hotkey_mgr.clone());

            // NOW spawn the capture controller with the hotkey receiver.
            let capture = capture::CaptureController::spawn(
                audio_dir.clone(),
                db.clone(),
                active_tab.clone(),
                mode_handle.clone(),
                hotkey_mgr.subscribe(),
            );
            let utt_rx = capture.utterance_receiver();
            app.manage(capture);

            // ...existing supervisor + drainer wiring unchanged...
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn load_capture_mode(db: &Db) -> Option<capture::CaptureMode> {
    db::settings::get::<String>(db, "capture_mode")
        .ok()
        .flatten()
        .and_then(|s| capture::CaptureMode::parse(&s))
}
```

(`audio_dir` is now consumed inside `setup()` — move its declaration into the closure or `clone()` it before the closure.)

- [ ] **Step 2: `cargo build` to confirm the surgery compiles**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

Fix the inevitable "borrow moved into closure" issues by cloning before the closure (`let db_for_setup = db.clone();`).

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/lib.rs
git commit -m "feat(lib): mount HotkeyManager + CaptureMode; spawn controller in setup"
```

---

## Task 8: Tauri commands — `capture_set_mode`, `hotkey_*`

**Files:**
- Modify: `src-tauri/src/commands/capture.rs`
- Create: `src-tauri/src/commands/hotkey.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/commands/settings.rs` (notice `capture_mode` / `hotkey_binding` writes and rewire)

- [ ] **Step 1: Add mode commands to `capture.rs`**

```rust
use crate::capture::{CaptureMode, CaptureModeHandle};

#[tauri::command]
pub fn capture_get_mode(handle: State<'_, CaptureModeHandle>) -> Result<String, CommandError> {
    Ok(handle.get().as_str().to_string())
}

#[tauri::command]
pub fn capture_set_mode(
    handle: State<'_, CaptureModeHandle>,
    db: State<'_, crate::db::Db>,
    mode: String,
) -> Result<(), CommandError> {
    let parsed = CaptureMode::parse(&mode)
        .ok_or_else(|| CommandError::Validation(format!("unknown capture_mode: {mode}")))?;
    handle.set(parsed);
    crate::db::settings::set(&db, "capture_mode", &parsed.as_str().to_string())
        .map_err(|e| CommandError::Storage(e.to_string()))?;
    Ok(())
}
```

- [ ] **Step 2: Create `commands/hotkey.rs`**

```rust
use std::time::Duration;

use tauri::State;

use crate::hotkey::{Binding, HotkeyManager};
use crate::db::Db;

use super::tabs::CommandError;

#[tauri::command]
pub fn hotkey_get_binding(mgr: State<'_, HotkeyManager>) -> Result<Option<Binding>, CommandError> {
    Ok(mgr.current_binding())
}

#[tauri::command]
pub fn hotkey_set_binding(
    mgr: State<'_, HotkeyManager>,
    db: State<'_, Db>,
    binding: Binding,
) -> Result<(), CommandError> {
    mgr.set_binding(binding.clone())
        .map_err(|e| CommandError::Storage(e.to_string()))?;
    crate::db::settings::set(&db, "hotkey_binding", &binding)
        .map_err(|e| CommandError::Storage(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn hotkey_clear_binding(
    mgr: State<'_, HotkeyManager>,
    db: State<'_, Db>,
) -> Result<(), CommandError> {
    mgr.clear_binding();
    let _ = crate::db::settings::delete(&db, "hotkey_binding");
    Ok(())
}

#[tauri::command]
pub fn hotkey_capture_next(mgr: State<'_, HotkeyManager>) -> Result<Binding, CommandError> {
    mgr.capture_next_press(Duration::from_secs(15))
        .map_err(|e| CommandError::Validation(e.to_string()))
}
```

> If `db::settings::delete` doesn't exist, add a `pub fn delete(db: &Db, key: &str) -> Result<(), rusqlite::Error>` to `db/settings.rs` (simple `DELETE FROM settings WHERE key = ?`).

- [ ] **Step 3: Add `pub mod hotkey;` to `commands/mod.rs`**

- [ ] **Step 4: Run sanity build**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/commands
git commit -m "feat(commands): capture_set_mode + hotkey_* tauri commands"
```

---

## Task 9: Tray icon — build, menu, window close-to-tray

**Files:**
- Create: `src-tauri/src/tray/mod.rs`
- Create: `src-tauri/src/tray/menu.rs`
- Create: `src-tauri/icons/tray-idle.png` (32x32 gray dot)
- Create: `src-tauri/icons/tray-capturing.png` (32x32 green dot)
- Modify: `src-tauri/src/lib.rs` (build the tray inside `setup()`, install the window event hook)

The icon files can be generated with a tiny Python or ImageMagick one-liner. Acceptable v1 quality: 32×32 PNG, a solid filled circle of the right color centered on transparent background. Keep them under 2 KB each.

- [ ] **Step 1: Generate the two PNGs**

```powershell
# Using ImageMagick (preferred if installed)
magick -size 32x32 xc:transparent -fill "#444" -draw "circle 16,16 16,26" src-tauri/icons/tray-idle.png
magick -size 32x32 xc:transparent -fill "#3a8a3a" -draw "circle 16,16 16,26" src-tauri/icons/tray-capturing.png
```

If ImageMagick is not on PATH, any 32×32 PNG with a recognizable gray and green dot is fine. Check both files into git.

- [ ] **Step 2: Create `src-tauri/src/tray/mod.rs`**

```rust
//! System tray icon + menu.
//!
//! Built once in `lib.rs::run()::setup()`. Owns:
//! - The tray icon (color reflects capture state).
//! - The menu (Show/Hide, Capture toggle, Mode toggle, Quit).
//! - The `should_exit` flag — when Quit is clicked we set it before
//!   `app.exit(0)`; the main window's `on_window_event` checks this flag and
//!   only `prevent_close()` if the flag is false (close → hide to tray).

pub mod menu;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub use menu::MenuIds;

#[derive(Clone)]
pub struct TrayHandle {
    icon: TrayIcon,
    should_exit: Arc<AtomicBool>,
}

impl TrayHandle {
    pub fn build(app: &AppHandle) -> tauri::Result<Self> {
        let should_exit = Arc::new(AtomicBool::new(false));
        let menu = menu::build_menu(app)?;
        let icon = TrayIconBuilder::with_id("voicetabs-tray")
            .icon(load_idle_icon(app)?)
            .menu(&menu)
            .menu_on_left_click(false) // we handle left-click ourselves
            .on_menu_event(menu::menu_event_handler(should_exit.clone()))
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                    toggle_main_window(tray.app_handle());
                }
            })
            .build(app)?;
        Ok(Self { icon, should_exit })
    }

    pub fn set_capturing(&self, capturing: bool) -> tauri::Result<()> {
        let app = self.icon.app_handle();
        let img = if capturing {
            load_capturing_icon(app)?
        } else {
            load_idle_icon(app)?
        };
        self.icon.set_icon(Some(img))
    }

    pub fn should_exit(&self) -> Arc<AtomicBool> {
        self.should_exit.clone()
    }
}

fn load_idle_icon(app: &AppHandle) -> tauri::Result<Image<'_>> {
    let bytes = include_bytes!("../../icons/tray-idle.png");
    Image::from_bytes(bytes)
}

fn load_capturing_icon(app: &AppHandle) -> tauri::Result<Image<'_>> {
    let bytes = include_bytes!("../../icons/tray-capturing.png");
    Image::from_bytes(bytes)
}

pub fn toggle_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}
```

- [ ] **Step 3: Create `src-tauri/src/tray/menu.rs`**

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Manager};

use crate::capture::{CaptureController, CaptureMode, CaptureModeHandle};

pub struct MenuIds;
impl MenuIds {
    pub const TOGGLE_WINDOW: &'static str = "tray:toggle_window";
    pub const TOGGLE_CAPTURE: &'static str = "tray:toggle_capture";
    pub const TOGGLE_MODE: &'static str = "tray:toggle_mode";
    pub const QUIT: &'static str = "tray:quit";
}

pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    // Labels are English fallbacks; in v1 the tray menu is built once at
    // startup and not re-rendered on locale change. Switching language updates
    // the in-app UI but not the tray — documented limitation.
    let toggle = MenuItem::with_id(app, MenuIds::TOGGLE_WINDOW, "Show / Hide", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, MenuIds::TOGGLE_CAPTURE, "Capture: off", true, None::<&str>)?;
    let mode = MenuItem::with_id(app, MenuIds::TOGGLE_MODE, "Mode: always on", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MenuIds::QUIT, "Quit", true, None::<&str>)?;
    Menu::with_items(app, &[&toggle, &capture, &mode, &sep, &quit])
}

pub fn menu_event_handler(
    should_exit: Arc<AtomicBool>,
) -> impl Fn(&AppHandle, MenuEvent) + Send + Sync + 'static {
    move |app, event| match event.id().as_ref() {
        MenuIds::TOGGLE_WINDOW => crate::tray::toggle_main_window(app),
        MenuIds::TOGGLE_CAPTURE => {
            if let Some(ctrl) = app.try_state::<CaptureController>() {
                // Toggle: if currently capturing, stop; else start.
                let capturing = !matches!(ctrl.status(), crate::capture::CaptureStatus::Idle);
                if capturing { ctrl.stop(); } else { ctrl.start(); }
            }
        }
        MenuIds::TOGGLE_MODE => {
            if let Some(handle) = app.try_state::<CaptureModeHandle>() {
                let next = match handle.get() {
                    CaptureMode::AlwaysOn => CaptureMode::Ptt,
                    CaptureMode::Ptt => CaptureMode::AlwaysOn,
                };
                handle.set(next);
                if let Some(db) = app.try_state::<crate::db::Db>() {
                    let _ = crate::db::settings::set(&db, "capture_mode", &next.as_str().to_string());
                }
            }
        }
        MenuIds::QUIT => {
            should_exit.store(true, Ordering::SeqCst);
            app.exit(0);
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Build the tray + install close-to-tray in `lib.rs::setup()`**

After the capture controller is mounted:

```rust
let tray = tray::TrayHandle::build(&app_handle).expect("build tray icon");
let should_exit = tray.should_exit();
app.manage(tray.clone());

// Intercept close events on the main window.
if let Some(window) = app_handle.get_webview_window("main") {
    let should_exit_for_window = should_exit.clone();
    window.on_window_event(move |evt| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = evt {
            if !should_exit_for_window.load(std::sync::atomic::Ordering::SeqCst) {
                api.prevent_close();
                if let Some(w) = app_handle.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
        }
    });
}

// Reflect capture state on the tray icon. The capture controller doesn't
// publish a stream of status changes today; we poll cheaply once a second.
// Phase 6 may replace this with an event.
{
    let tray_for_poll = tray.clone();
    let app_for_poll = app_handle.clone();
    std::thread::Builder::new()
        .name("voicetabs-tray-poll".into())
        .spawn(move || loop {
            let capturing = app_for_poll
                .try_state::<CaptureController>()
                .map(|c| !matches!(c.status(), crate::capture::CaptureStatus::Idle))
                .unwrap_or(false);
            let _ = tray_for_poll.set_capturing(capturing);
            std::thread::sleep(std::time::Duration::from_millis(1000));
        })
        .expect("spawn tray poll thread");
}
```

- [ ] **Step 5: Build + manually verify**

```powershell
cargo build --manifest-path src-tauri\Cargo.toml
```

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/tray src-tauri/icons/tray-idle.png src-tauri/icons/tray-capturing.png src-tauri/src/lib.rs
git commit -m "feat(tray): tray icon + menu + close-to-tray + status polling"
```

---

## Task 10: Frontend — `hotkeyApi`, `captureModeApi`, types

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add types + helpers**

```typescript
// --- Hotkey ---
export type BindingKind = "key" | "mouse";
export type Binding = { kind: BindingKind; code: string };

export const hotkeyApi = {
  async get(): Promise<Binding | null> {
    return await invoke<Binding | null>("hotkey_get_binding");
  },
  async set(binding: Binding): Promise<void> {
    await invoke("hotkey_set_binding", { binding });
  },
  async clear(): Promise<void> {
    await invoke("hotkey_clear_binding");
  },
  async captureNext(): Promise<Binding> {
    // Blocks on the backend for up to 15s; the UI shows "Pressione…" until
    // it resolves or throws.
    return await invoke<Binding>("hotkey_capture_next");
  },
};

// --- Capture mode ---
export type CaptureMode = "always_on" | "ptt";

export const captureModeApi = {
  async get(): Promise<CaptureMode> {
    return (await invoke<string>("capture_get_mode")) as CaptureMode;
  },
  async set(mode: CaptureMode): Promise<void> {
    await invoke("capture_set_mode", { mode });
  },
};
```

- [ ] **Step 2: Commit**

```powershell
git add src/lib/tauri.ts
git commit -m "feat(frontend): tauri.ts hotkey + capture mode API surfaces"
```

---

## Task 11: `captureStore` — mode + binding

**Files:**
- Modify: `src/stores/captureStore.ts`

- [ ] **Step 1: Add state + actions**

```typescript
import { create } from "zustand";
import { Binding, CaptureMode, captureModeApi, hotkeyApi, captureApi } from "../lib/tauri";

type State = {
  // existing capture on/off state...
  captureMode: CaptureMode;
  hotkeyBinding: Binding | null;

  loadMode: () => Promise<void>;
  setMode: (m: CaptureMode) => Promise<void>;

  loadBinding: () => Promise<void>;
  setBinding: (b: Binding) => Promise<void>;
  clearBinding: () => Promise<void>;
  captureNextBinding: () => Promise<Binding>;
};

export const useCaptureStore = create<State>((set, get) => ({
  // ...existing fields...
  captureMode: "always_on",
  hotkeyBinding: null,

  async loadMode() {
    const m = await captureModeApi.get();
    set({ captureMode: m });
  },
  async setMode(m) {
    await captureModeApi.set(m);
    set({ captureMode: m });
  },

  async loadBinding() {
    const b = await hotkeyApi.get();
    set({ hotkeyBinding: b });
  },
  async setBinding(b) {
    await hotkeyApi.set(b);
    set({ hotkeyBinding: b });
  },
  async clearBinding() {
    await hotkeyApi.clear();
    set({ hotkeyBinding: null });
  },
  async captureNextBinding() {
    const b = await hotkeyApi.captureNext();
    await hotkeyApi.set(b);
    set({ hotkeyBinding: b });
    return b;
  },
}));
```

Call `loadMode()` + `loadBinding()` at app startup (in `App.tsx`'s effect alongside the existing `useTabsStore.load()`).

- [ ] **Step 2: Commit**

```powershell
git add src/stores/captureStore.ts src/App.tsx
git commit -m "feat(frontend): captureStore mode + binding state"
```

---

## Task 12: `CaptureSettings.tsx` — mode dropdown + hotkey row + `HotkeyBinder`

**Files:**
- Create: `src/components/CaptureSettings.tsx`
- Create: `src/components/HotkeyBinder.tsx`
- Modify: `src/components/SettingsDrawer.tsx` (render `<CaptureSettings />` above `<VocabSettings />`)

- [ ] **Step 1: Create `src/components/CaptureSettings.tsx`**

```tsx
import { useTranslation } from "react-i18next";

import { useCaptureStore } from "../stores/captureStore";
import { CaptureMode } from "../lib/tauri";
import { HotkeyBinder } from "./HotkeyBinder";

export function CaptureSettings() {
  const { t } = useTranslation();
  const { captureMode, setMode, hotkeyBinding } = useCaptureStore();

  return (
    <section className="drawer__section">
      <h3>{t("settings.captureHeading")}</h3>
      <label htmlFor="capture-mode-select">{t("settings.captureMode")}</label>
      <select
        id="capture-mode-select"
        value={captureMode}
        onChange={(e) => void setMode(e.target.value as CaptureMode)}
      >
        <option value="always_on">{t("settings.captureModeAlwaysOn")}</option>
        <option value="ptt">{t("settings.captureModePtt")}</option>
      </select>

      <div className="hotkey-row">
        <span className="hotkey-row__label">{t("settings.hotkey")}</span>
        <span className="hotkey-row__current" data-testid="hotkey-current">
          {hotkeyBinding ? humanLabel(hotkeyBinding) : t("settings.hotkeyNone")}
        </span>
        <HotkeyBinder />
      </div>
    </section>
  );
}

function humanLabel(b: { kind: string; code: string }): string {
  if (b.kind === "mouse") {
    if (b.code === "MouseButton4") return "Mouse Button 4";
    if (b.code === "MouseButton5") return "Mouse Button 5";
    return b.code;
  }
  // Keyboard: a tiny in-frontend lookup so we don't roundtrip to Rust.
  const map: Record<string, string> = {
    ControlLeft: "Left Ctrl",
    ControlRight: "Right Ctrl",
    ShiftLeft: "Left Shift",
    ShiftRight: "Right Shift",
    AltLeft: "Left Alt",
    AltRight: "Right Alt",
    Space: "Space",
  };
  return map[b.code] ?? b.code;
}
```

- [ ] **Step 2: Create `src/components/HotkeyBinder.tsx`**

```tsx
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useCaptureStore } from "../stores/captureStore";

export function HotkeyBinder() {
  const { t } = useTranslation();
  const { captureNextBinding } = useCaptureStore();
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onBind() {
    setError(null);
    setCapturing(true);
    try {
      await captureNextBinding();
    } catch (e) {
      const msg = (e as { message?: string })?.message ?? String(e);
      // Cancelled / timed-out are not errors worth red text.
      if (msg.includes("cancelled") || msg.includes("timed out")) {
        // no-op
      } else {
        setError(msg);
      }
    } finally {
      setCapturing(false);
    }
  }

  if (capturing) {
    return (
      <span role="status" data-testid="hotkey-capturing">
        {t("settings.hotkeyCapturing")}
        <button onClick={() => setCapturing(false)} type="button">
          {t("settings.hotkeyCancel")}
        </button>
      </span>
    );
  }

  return (
    <>
      <button onClick={onBind} type="button">
        {t("settings.hotkeyBind")}
      </button>
      {error && <span className="hotkey-error">{error}</span>}
    </>
  );
}
```

> **Cancellation note.** Clicking "Cancelar" in the UI only clears the frontend's `capturing` state; the backend's `capture_next_press` still has up to 15 s to time out, during which a key press would still be consumed. That's acceptable for v1 — the user can press Esc to fire the backend's `Cancelled` path, which resolves the promise instantly. Phase 6 may add an explicit `hotkey_capture_cancel` command.

- [ ] **Step 3: Render `<CaptureSettings />` in `SettingsDrawer.tsx`**

```tsx
import { CaptureSettings } from "./CaptureSettings";

// ...inside the drawer body, above <VocabSettings />:
<CaptureSettings />
<VocabSettings />
```

- [ ] **Step 4: Commit**

```powershell
git add src/components/CaptureSettings.tsx src/components/HotkeyBinder.tsx src/components/SettingsDrawer.tsx
git commit -m "feat(ui): capture settings drawer section with hotkey binder"
```

---

## Task 13: i18n keys for capture + hotkey + tray

**Files:**
- Modify: `src/i18n/locales/pt-BR.json`
- Modify: `src/i18n/locales/en.json`

- [ ] **Step 1: Add keys to `pt-BR.json`**

Under `"settings"`:

```json
"captureHeading": "Captura",
"captureMode": "Modo",
"captureModeAlwaysOn": "Sempre ativo",
"captureModePtt": "Push-to-talk",
"hotkey": "Atalho",
"hotkeyNone": "Nenhum",
"hotkeyBind": "Vincular",
"hotkeyCapturing": "Pressione qualquer tecla ou botão…",
"hotkeyCancel": "Cancelar"
```

Add a top-level `"tray"` key:

```json
"tray": {
  "show": "Mostrar VoiceTabs",
  "hide": "Ocultar VoiceTabs",
  "captureOn": "Captura: ligada",
  "captureOff": "Captura: desligada",
  "modeAlwaysOn": "Modo: sempre ativo",
  "modePtt": "Modo: push-to-talk",
  "quit": "Sair"
}
```

- [ ] **Step 2: Same keys in `en.json`**

```json
"captureHeading": "Capture",
"captureMode": "Mode",
"captureModeAlwaysOn": "Always on",
"captureModePtt": "Push-to-talk",
"hotkey": "Hotkey",
"hotkeyNone": "None",
"hotkeyBind": "Bind",
"hotkeyCapturing": "Press any key or button…",
"hotkeyCancel": "Cancel"
```

```json
"tray": {
  "show": "Show VoiceTabs",
  "hide": "Hide VoiceTabs",
  "captureOn": "Capture: on",
  "captureOff": "Capture: off",
  "modeAlwaysOn": "Mode: always on",
  "modePtt": "Mode: push-to-talk",
  "quit": "Quit"
}
```

- [ ] **Step 3: Verify the existing i18n test still passes**

```powershell
npm test -- i18n
```

If `i18n.test.tsx` asserts symmetry between the two locale bundles, all new keys are required in both — already covered above.

- [ ] **Step 4: Commit**

```powershell
git add src/i18n/locales
git commit -m "i18n: capture mode + hotkey + tray keys (pt-BR + en)"
```

---

## Task 14: TS tests — `CaptureSettings`, `HotkeyBinder`, settings round-trip

**Files:**
- Create: `src/__tests__/CaptureSettings.test.tsx`
- Create: `src/__tests__/HotkeyBinder.test.tsx`
- Modify: `src/__tests__/settingsStore.test.ts` (or create if absent)

- [ ] **Step 1: `CaptureSettings.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { CaptureSettings } from "../components/CaptureSettings";
import { useCaptureStore } from "../stores/captureStore";

vi.mock("../lib/tauri", () => ({
  captureModeApi: { get: vi.fn().mockResolvedValue("always_on"), set: vi.fn().mockResolvedValue(undefined) },
  hotkeyApi: { get: vi.fn().mockResolvedValue(null), set: vi.fn(), clear: vi.fn(), captureNext: vi.fn() },
}));

beforeEach(() => {
  useCaptureStore.setState({ captureMode: "always_on", hotkeyBinding: null });
});

describe("CaptureSettings", () => {
  it("renders the current mode and updates the store on change", async () => {
    render(<CaptureSettings />);
    const select = screen.getByLabelText(/mode/i) as HTMLSelectElement;
    expect(select.value).toBe("always_on");
    await userEvent.selectOptions(select, "ptt");
    expect(useCaptureStore.getState().captureMode).toBe("ptt");
  });

  it("shows 'None' when no hotkey is bound", () => {
    render(<CaptureSettings />);
    expect(screen.getByTestId("hotkey-current").textContent).toMatch(/none|nenhum/i);
  });

  it("shows the human label when a binding exists", () => {
    useCaptureStore.setState({
      captureMode: "ptt",
      hotkeyBinding: { kind: "key", code: "ControlRight" },
    });
    render(<CaptureSettings />);
    expect(screen.getByTestId("hotkey-current").textContent).toMatch(/right ctrl/i);
  });
});
```

- [ ] **Step 2: `HotkeyBinder.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { HotkeyBinder } from "../components/HotkeyBinder";
import { useCaptureStore } from "../stores/captureStore";

vi.mock("../lib/tauri", () => ({
  hotkeyApi: {
    get: vi.fn(),
    set: vi.fn(),
    clear: vi.fn(),
    captureNext: vi.fn(),
  },
  captureModeApi: { get: vi.fn(), set: vi.fn() },
}));

import { hotkeyApi } from "../lib/tauri";

beforeEach(() => {
  vi.clearAllMocks();
  useCaptureStore.setState({ captureMode: "ptt", hotkeyBinding: null });
});

describe("HotkeyBinder", () => {
  it("shows 'Bind' by default and switches to capturing on click", async () => {
    let resolveCapture: (b: { kind: "key"; code: string }) => void;
    vi.mocked(hotkeyApi.captureNext).mockReturnValueOnce(
      new Promise((res) => { resolveCapture = res as never; })
    );
    render(<HotkeyBinder />);
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("hotkey-capturing")).toBeInTheDocument();
    resolveCapture!({ kind: "key", code: "Space" });
  });

  it("calls captureNextBinding and updates the store on success", async () => {
    vi.mocked(hotkeyApi.captureNext).mockResolvedValueOnce({ kind: "key", code: "ControlRight" });
    render(<HotkeyBinder />);
    await userEvent.click(screen.getByRole("button"));
    // Allow the promise chain to resolve.
    await new Promise((r) => setTimeout(r, 0));
    expect(useCaptureStore.getState().hotkeyBinding?.code).toBe("ControlRight");
  });

  it("treats backend 'cancelled' errors as silent", async () => {
    vi.mocked(hotkeyApi.captureNext).mockRejectedValueOnce(new Error("capture cancelled"));
    render(<HotkeyBinder />);
    await userEvent.click(screen.getByRole("button"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByText(/cancelled/)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run TS tests**

```powershell
npm test
```

Expected: ~10 new passing tests; no regressions.

- [ ] **Step 4: Commit**

```powershell
git add src/__tests__/CaptureSettings.test.tsx src/__tests__/HotkeyBinder.test.tsx
git commit -m "test(ui): CaptureSettings + HotkeyBinder coverage"
```

---

## Task 15: Rust integration — PTT path end-to-end with mock backends

The keyboard / mouse backends can't be exercised in unit tests (no message pump in `cargo test`). But the controller's PTT gating IS testable: we instantiate `CaptureModeHandle` + a crossbeam channel that mocks `HotkeyManager::subscribe`, push synthetic press/release events, and assert the right utterance lands.

**Files:**
- Create: `src-tauri/tests/ptt_gating_test.rs`

- [ ] **Step 1: Create the integration test**

```rust
//! PTT path: a manually-driven hotkey channel produces an utterance with the
//! right tab id, even when VAD probabilities never cross the thresholds.

use std::time::Duration;

use voicetabs_lib::capture::{CaptureController, CaptureMode, CaptureModeHandle};
use voicetabs_lib::db;
use voicetabs_lib::hotkey::HotkeyEvent;
use voicetabs_lib::routing::ActiveTab;

#[test]
#[ignore = "requires audio device; run manually with --ignored"]
fn ptt_press_release_emits_utterance() {
    // This test is `#[ignore]` because the capture worker needs a real cpal
    // input device. The CI runner doesn't have one. Run locally with:
    //   cargo test --manifest-path src-tauri/Cargo.toml -- --ignored ptt_press_release_emits_utterance
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = db::open(&db_path).unwrap();
    db::tabs::insert_with_title(&db, "T1").unwrap();
    let active = ActiveTab::new();
    active.set(Some(1));
    let mode = CaptureModeHandle::new(CaptureMode::Ptt);

    let (hk_tx, hk_rx) = crossbeam_channel::unbounded::<HotkeyEvent>();
    let ctrl = CaptureController::spawn(
        tmp.path().join("audio"),
        db.clone(),
        active.clone(),
        mode.clone(),
        hk_rx,
    );
    ctrl.start();
    std::thread::sleep(Duration::from_millis(300));

    hk_tx.send(HotkeyEvent::Press).unwrap();
    std::thread::sleep(Duration::from_millis(800)); // speak window
    hk_tx.send(HotkeyEvent::Release).unwrap();

    let utterance = ctrl.utterance_receiver().recv_timeout(Duration::from_secs(2));
    assert!(utterance.is_ok(), "expected utterance within 2s of release, got {utterance:?}");
    let u = utterance.unwrap();
    assert_eq!(u.start_tab_id, 1);
    assert!(u.samples.len() > 0, "utterance should have buffered samples");
}
```

- [ ] **Step 2: Run with `--ignored`**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml -- --ignored ptt_press_release_emits_utterance
```

If you don't have a mic plugged into the CI machine, document the test as manually-run and move on.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/tests/ptt_gating_test.rs
git commit -m "test(capture): manual PTT integration drill"
```

---

## Task 16: Type-check + full test sweep

- [ ] **Step 1: Frontend type-check**

```powershell
npx tsc --noEmit
```

- [ ] **Step 2: `cargo test`**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected counts after Phase 5 additions:
- hotkey::binding — 5
- hotkey::mouse — 2
- capture::mode — 5
- vad::state (force_speaking) — +1
- existing — unchanged

- [ ] **Step 3: `npm test`**

```powershell
npm test
```

Expected: green; Phase 4 baseline + ~10 Phase 5 tests.

- [ ] **Step 4: Commit (empty allowed)**

```powershell
git diff --stat
git commit --allow-empty -m "chore: full test sweep passes for Phase 5"
```

---

## Task 17: Manual acceptance — A4 (PTT works minimized/unfocused)

This is the Phase 5 acceptance gate. A4 cannot be unit-tested; the OS itself is the system under test.

- [ ] **Step 1: Launch the app**

```powershell
npm run tauri dev
```

- [ ] **Step 2: Bind a keyboard hotkey**

1. Open **Configurações → Captura**.
2. Set mode to **Push-to-talk**.
3. Click **Vincular**. The button label changes to **Pressione qualquer tecla ou botão…**.
4. Press the **Right Ctrl** key.
5. Confirm the label updates to "Right Ctrl" / "Right Ctrl". Close the drawer.

- [ ] **Step 3: A4 case 1 — app focused**

1. Click any tab.
2. Hold Right Ctrl, speak a short sentence ("Teste com a tecla direita."), release.
3. Wait ≤ 1.5 s. A segment card lands under the active tab.

Expected: pass.

- [ ] **Step 4: A4 case 2 — app minimized**

1. Click the OS minimize button on the main window.
2. While the window is minimized, hold Right Ctrl, speak ("Falando com a janela minimizada."), release.
3. Click the tray icon to restore the window.

Expected: the segment card from step 2 is in the active tab.

- [ ] **Step 5: A4 case 3 — app unfocused (focus another window)**

1. Open Notepad. Click into it so it has focus.
2. Hold Right Ctrl, speak ("Estou em outro aplicativo."), release.
3. Switch back to VoiceTabs.

Expected: the segment card is in the active tab.

> **If step 5 fails** (no segment, or Notepad receives the keystrokes instead of the hotkey firing): the `tauri-plugin-global-shortcut` registration likely silently failed because another app already claimed `RightControl`. Pick a different key (`F13` works on most keyboards even without a physical F13 — Windows generates the scancode), re-bind, and retry.

- [ ] **Step 6: A4 case 4 — mouse button**

1. Open **Configurações → Captura → Vincular**. Press **Mouse Button 5** (the front side button on most mice).
2. Confirm the label is "Mouse Button 5".
3. Focus Notepad, hold MB5, speak ("Botão lateral."), release.
4. Switch to VoiceTabs.

Expected: segment card present. If the mouse button isn't recognized, verify with a tool like `xev`-equivalent (e.g. AutoHotkey's `KeyHistory`) that Windows sees the event; the hook should observe any `WM_XBUTTONDOWN`.

- [ ] **Step 7: Mode hot-reload**

1. With capture ON and mode = PTT, change the dropdown to "Sempre ativo".
2. Without pressing the hotkey, speak. A segment should land (VAD edges now drive boundaries).
3. Change back to PTT mid-speech.
4. Speak without holding the hotkey — no segment.
5. Hold the hotkey + speak + release — segment lands.

Expected: each mode transition is honored immediately, no app restart needed.

- [ ] **Step 8: Tray — close to tray + restore**

1. Click the OS × button on the main window.
2. The window disappears; the process is still running (check Task Manager: `voicetabs.exe`).
3. Left-click the tray icon. Window restores and gains focus.
4. Tray right-click → **Sair**. The process exits.

Expected: every step behaves as described.

- [ ] **Step 9: Tray menu actions**

1. Restart the app.
2. Right-click the tray icon → **Capture: off** → confirm the footer toggles ON.
3. Right-click → **Mode: always on** → confirm settings drawer dropdown matches.
4. Right-click → **Show / Hide** → window toggles.

- [ ] **Step 10: Stop-and-report**

If every step passed, A4 is met. Write the report covering:
- Bound key and mouse button used.
- Observed latency for cases 1–4 (eyeball).
- Any plugin-registration failures and how you worked around them.
- Tray menu polish issues to revisit in Phase 6 (e.g. menu labels not re-rendering on locale change — known limitation, documented).

- [ ] **Step 11: Final commit (if any tweaks fell out)**

```powershell
git diff --stat
git commit -am "chore: phase 5 manual acceptance pass"
```

---

## End of Phase 5 — checkpoint

**Stop here and produce a stop-and-report.** Phase 6 (segment polish + vocab UX) is planned in a separate session.

**What's verified at this checkpoint:**
- A1 — not yet (installer in Phase 8).
- A2 — Phase 3.
- A3 — Phase 4 (unaffected here).
- A4 — **full**. PTT works in all three scenarios (focused, minimized, unfocused), with both keyboard and mouse bindings.
- A5 — Phase 2/4 (unaffected here).
- A6 — Phase 4 (unaffected here).
- A7 — partial (Phase 7 will revisit recovery latency).
- A8 — Phase 4; Phase 5 adds `capture_mode` and `hotkey_binding` settings keys, both of which persist via the existing settings repo.
- L1, L2 — inherited from Phase 4.

**What's verified that isn't on the criteria list:**
- The `WH_MOUSE_LL` hook is read-only and chains every event.
- The mouse backend filters out left/right/middle buttons so the user cannot brick the UI with their primary input.
- Capture mode switches mid-utterance gracefully: an in-flight always-on utterance finalizes normally even if the user switches to PTT before it ends (the controller's `current_meta` is consumed by whichever path finalizes first).
- Closing the main window via × hides to tray; the process keeps running and resumes immediately on tray-click.
- The tray icon updates its color once per second based on `CaptureStatus`. Acceptable for v1; Phase 6 may push events instead.
- All new `tauri-plugin-*` dependencies are listed in `Cargo.toml`, registered in `tauri.conf.json`, and granted capability permissions in `capabilities/default.json`.

**Known limitations carried forward:**
- Tray menu labels are built once at startup; switching the UI locale at runtime does not re-render the tray menu. Re-launching the app picks up the new labels. Tracked for Phase 6.
- The keyboard capture-mode listener registers a small fixed set of common PTT keys (Ctrl/Shift/Alt L/R, Space, F13–F19, Escape). Other keys cannot be bound from the UI in v1; the user can still set any key via the settings JSON by hand. Phase 6 may expand the set or hook the plugin's raw-key listener.
- `hotkey_capture_next` has a 15 s server-side timeout; the frontend "Cancel" button only updates the UI — the backend still consumes the next press unless the user presses Esc. Phase 6 may add an explicit cancel command.
