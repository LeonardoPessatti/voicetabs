//! Mono downmix + linear-interpolation resampler.
//!
//! This is intentionally simple. cpal frames arrive at the device's native
//! rate (commonly 44.1 kHz or 48 kHz) and channel count (commonly 1 or 2).
//! Silero VAD wants mono 16 kHz. Linear interpolation introduces mild
//! aliasing but the VAD model is trained to be robust to it; the resulting
//! debug WAVs are not high-fidelity but are perfectly intelligible.
//!
//! If we ever need broadcast-quality WAVs, swap this for `rubato`.

/// Stateless per-frame downmix + linear resampler. The function processes one
/// cpal frame at a time. It is intentionally stateless across calls — frame
/// boundaries may produce tiny discontinuities (negligible for VAD).
pub fn downmix_and_resample(
    frame: &[f32],
    source_rate: u32,
    source_channels: u16,
    target_rate: u32,
) -> Vec<f32> {
    // 1. Downmix to mono.
    let mono: Vec<f32> = if source_channels > 1 {
        let ch = source_channels as usize;
        frame
            .chunks_exact(ch)
            .map(|c| c.iter().sum::<f32>() / ch as f32)
            .collect()
    } else {
        frame.to_vec()
    };

    // 2. Resample.
    if source_rate == target_rate || mono.is_empty() {
        return mono;
    }
    let ratio = target_rate as f64 / source_rate as f64;
    let out_len = (mono.len() as f64 * ratio).floor() as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let step = 1.0 / ratio; // input samples per output sample
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * step;
        let src_idx = src_pos.floor() as usize;
        let frac = src_pos.fract() as f32;
        if src_idx + 1 < mono.len() {
            output.push(mono[src_idx] + frac * (mono[src_idx + 1] - mono[src_idx]));
        } else if src_idx < mono.len() {
            output.push(mono[src_idx]);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_rates_match_and_mono() {
        let input = vec![0.1, 0.2, 0.3, 0.4];
        let out = downmix_and_resample(&input, 16_000, 1, 16_000);
        assert_eq!(out, input);
    }

    #[test]
    fn downmix_stereo_averages_channels() {
        // Interleaved stereo: L0 R0 L1 R1
        let input = vec![1.0, 0.0, 0.0, 1.0];
        let out = downmix_and_resample(&input, 16_000, 2, 16_000);
        // Both pairs average to 0.5.
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn downsample_48k_to_16k_produces_one_third_length() {
        // 48 kHz mono, 3000 samples ≈ 62.5 ms. Resampling to 16 kHz should
        // give ~1000 samples.
        let input: Vec<f32> = (0..3_000).map(|i| (i as f32 / 1000.0).sin()).collect();
        let out = downmix_and_resample(&input, 48_000, 1, 16_000);
        // Allow ±1 due to floor()-based out_len computation.
        assert!(
            (999..=1001).contains(&out.len()),
            "got {} samples, expected ~1000",
            out.len()
        );
    }

    #[test]
    fn downmix_then_resample_combines_both_ops() {
        // 48 kHz stereo (3000 frames = 6000 interleaved samples) → 16 kHz mono.
        let input: Vec<f32> = (0..6_000)
            .map(|i| if i % 2 == 0 { 0.4 } else { 0.6 })
            .collect();
        let out = downmix_and_resample(&input, 48_000, 2, 16_000);
        assert!(
            (999..=1001).contains(&out.len()),
            "got {} samples, expected ~1000",
            out.len()
        );
        // Every input frame averages to 0.5; the resampled output is also 0.5
        // (allow tiny FP drift).
        for v in &out {
            assert!((v - 0.5).abs() < 1e-5, "got {v}");
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let out = downmix_and_resample(&[], 48_000, 2, 16_000);
        assert!(out.is_empty());
    }
}
