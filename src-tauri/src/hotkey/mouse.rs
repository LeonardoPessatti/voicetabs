//! Stub `MouseBackend`. Real implementation lands in Task 4 of Phase 5.

use crossbeam_channel::Sender;

use super::{Binding, BindingError, HotkeyEvent};

pub struct MouseBackend;

impl MouseBackend {
    pub fn new(_event_tx: Sender<HotkeyEvent>) -> anyhow::Result<Self> {
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
