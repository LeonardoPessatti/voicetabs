//! Stub `KeyboardBackend`. Real implementation lands in Task 3 of Phase 5.

use crossbeam_channel::Sender;

use super::{Binding, BindingError, HotkeyEvent};

pub struct KeyboardBackend;

impl KeyboardBackend {
    pub fn new(
        _app: tauri::AppHandle,
        _event_tx: Sender<HotkeyEvent>,
    ) -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub fn watch(&self, _code: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn unwatch(&self) {}

    pub fn start_capture(
        &self,
        _tx: Sender<Result<Binding, BindingError>>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn stop_capture(&self) {}
}
