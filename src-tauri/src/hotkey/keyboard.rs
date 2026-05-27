//! Keyboard hotkey backend wrapping `tauri-plugin-global-shortcut`.
//!
//! Two modes:
//! - **Watch** — a single `Shortcut` is registered; press/release events are
//!   forwarded to the manager's `event_tx`.
//! - **Capture** — listen for a small set of common PTT keys plus `Escape`
//!   (the latter so the user can always cancel from the keyboard). The first
//!   event becomes the new binding. We do not try to register every key on
//!   the keyboard — global-hotkey's API requires explicit registration per
//!   key. The mouse backend handles real first-press detection for users who
//!   want to bind anything outside this fallback set.

use std::str::FromStr;
use std::sync::Arc;

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

use super::{Binding, BindingError, HotkeyEvent};

/// Sender slot armed by `start_capture`; the first key event consumed by
/// the global-shortcut callback resolves to a `Binding` or `BindingError`.
type CaptureSlot = Arc<Mutex<Option<Sender<Result<Binding, BindingError>>>>>;

pub struct KeyboardBackend {
    app: AppHandle,
    event_tx: Sender<HotkeyEvent>,
    current: Arc<Mutex<Option<Shortcut>>>,
    capture_tx: CaptureSlot,
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
        plugin
            .on_shortcut(shortcut, move |_app, _shortcut, event| match event.state() {
                ShortcutState::Pressed => {
                    let _ = event_tx.send(HotkeyEvent::Press);
                }
                ShortcutState::Released => {
                    let _ = event_tx.send(HotkeyEvent::Release);
                }
            })
            .map_err(|e| anyhow::anyhow!("register global shortcut {code:?}: {e}"))?;
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
        // keyboard. We also register a small set of common PTT keys (left/
        // right ctrl, shift, alt, space, F13-F19); the user can bind a
        // mouse button to capture from the mouse backend for anything else.
        let common_codes = [
            Code::Escape,
            Code::ControlLeft,
            Code::ControlRight,
            Code::ShiftLeft,
            Code::ShiftRight,
            Code::AltLeft,
            Code::AltRight,
            Code::Space,
            Code::F13,
            Code::F14,
            Code::F15,
            Code::F16,
            Code::F17,
            Code::F18,
            Code::F19,
        ];
        *self.capture_tx.lock() = Some(tx);
        let capture_tx = self.capture_tx.clone();
        let plugin = self.app.global_shortcut();
        // Wipe any prior registration (active binding or a stale capture set
        // from a previous attempt) so the per-code register loop below can't
        // hit "already registered". `stop_capture` re-registers `current`.
        let _ = plugin.unregister_all();
        for code in common_codes {
            let shortcut = Shortcut::new(None, code);
            let capture_tx_for_handler = capture_tx.clone();
            plugin
                .on_shortcut(shortcut, move |_app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    // Only fire if capture is still armed; the manager clears
                    // `capture_tx` after the first send so subsequent presses
                    // are ignored.
                    let Some(tx) = capture_tx_for_handler.lock().take() else {
                        return;
                    };
                    if shortcut.key == Code::Escape {
                        let _ = tx.send(Err(BindingError::Cancelled));
                    } else {
                        let code_str = format!("{:?}", shortcut.key);
                        let _ = tx.send(Ok(Binding::key(code_str)));
                    }
                })
                .map_err(|e| anyhow::anyhow!("register capture shortcut {code:?}: {e}"))?;
        }
        Ok(())
    }

    pub fn stop_capture(&self) {
        *self.capture_tx.lock() = None;
        let plugin = self.app.global_shortcut();
        let _ = plugin.unregister_all();
        // Re-arm the current binding (caller `HotkeyManager::capture_next_press`
        // also does this; we belt-and-suspenders so double-stop is safe).
        if let Some(shortcut) = *self.current.lock() {
            let event_tx = self.event_tx.clone();
            let _ = plugin.on_shortcut(shortcut, move |_app, _s, event| match event.state() {
                ShortcutState::Pressed => {
                    let _ = event_tx.send(HotkeyEvent::Press);
                }
                ShortcutState::Released => {
                    let _ = event_tx.send(HotkeyEvent::Release);
                }
            });
        }
    }
}

fn parse_code(code: &str) -> Result<Shortcut, anyhow::Error> {
    // The plugin re-exports `Code` from `keyboard-types`, which impls FromStr
    // on the canonical KeyboardEvent.code strings (e.g. "ControlRight").
    let parsed: Code =
        Code::from_str(code).map_err(|e| anyhow::anyhow!("invalid key code {code:?}: {e}"))?;
    Ok(Shortcut::new(None, parsed))
}
