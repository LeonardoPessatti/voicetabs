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
    #[allow(dead_code)]
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
        if let Err(e) = self.inner.keyboard.start_capture(tx.clone()) {
            return Err(BindingError::InvalidKeyCode(e.to_string()));
        }
        if let Err(e) = self.inner.mouse.start_capture(tx) {
            self.inner.keyboard.stop_capture();
            return Err(BindingError::UnsupportedMouseButton(e.to_string()));
        }
        let result = rx
            .recv_timeout(timeout)
            .unwrap_or(Err(BindingError::Timeout));
        self.inner.keyboard.stop_capture();
        self.inner.mouse.stop_capture();
        // Re-arm the existing binding if one was set before capture began.
        if let Some(b) = self.current_binding() {
            let _ = self.set_binding(b);
        }
        result
    }
}
