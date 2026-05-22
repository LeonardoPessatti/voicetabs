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
