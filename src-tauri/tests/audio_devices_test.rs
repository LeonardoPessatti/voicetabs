//! Smoke test for cpal device enumeration. Does not assert on the contents
//! (CI runners may have no audio devices), only that the function returns
//! without panicking and produces well-formed structs.

#[test]
fn list_input_devices_does_not_panic() {
    let devices = voicetabs_lib::audio::list_input_devices();
    for d in &devices {
        assert!(!d.name.is_empty(), "device name must be non-empty");
    }
    // `is_default == true` for at most one device.
    let default_count = devices.iter().filter(|d| d.is_default).count();
    assert!(default_count <= 1, "got {default_count} defaults");
}

#[test]
fn default_input_name_does_not_panic() {
    let _ = voicetabs_lib::audio::default_input_name();
}
