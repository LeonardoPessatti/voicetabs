//! Capture mode — `AlwaysOn` (VAD edges drive boundaries) or `Ptt` (hotkey
//! drives boundaries).
//!
//! The handle is `Send + Sync + Clone`; the controller worker reads it on
//! every event. Writes happen from the `settings_set` command when the user
//! changes the dropdown.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    AlwaysOn,
    Ptt,
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
