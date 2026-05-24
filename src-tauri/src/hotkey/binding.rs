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
