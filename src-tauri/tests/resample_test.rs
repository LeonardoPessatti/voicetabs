//! Integration test verifying the resampler is callable through the public
//! re-export at `voicetabs_lib::audio::downmix_and_resample`.

use voicetabs_lib::audio::downmix_and_resample;

#[test]
fn public_reexport_is_callable() {
    let input = vec![0.5_f32; 4_800]; // 100 ms at 48 kHz mono.
    let out = downmix_and_resample(&input, 48_000, 1, 16_000);
    // Should be ~1600 samples (100 ms at 16 kHz).
    assert!((1_599..=1_601).contains(&out.len()), "got {}", out.len());
}
