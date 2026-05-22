//! Root-mean-square energy of a PCM f32 buffer expressed in dBFS.
//!
//! 0 dBFS = full-scale sine wave (amplitude 1.0). Silence returns a finite
//! floor of -100 dBFS rather than -infinity so callers can compare it
//! directly against thresholds. An empty buffer also returns -100.

/// Return the RMS of `samples` in dBFS, clamped to >= -100.
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let sum_sq: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    let mean_sq = sum_sq / samples.len() as f64;
    let rms = mean_sq.sqrt();
    if rms <= 1e-10 {
        return -100.0;
    }
    // 20 * log10(rms / 1.0). Reference = 1.0 full-scale.
    let db = 20.0 * rms.log10();
    (db as f32).max(-100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_floor() {
        assert_eq!(rms_dbfs(&[]), -100.0);
    }

    #[test]
    fn pure_silence_returns_floor() {
        let zeros = vec![0.0_f32; 1024];
        assert_eq!(rms_dbfs(&zeros), -100.0);
    }

    #[test]
    fn full_scale_dc_is_zero_dbfs() {
        // RMS of [1.0; N] is 1.0 → 0 dBFS.
        let ones = vec![1.0_f32; 1024];
        let db = rms_dbfs(&ones);
        assert!(db.abs() < 1e-3, "got {db}");
    }

    #[test]
    fn half_amplitude_sine_is_about_minus_9_dbfs() {
        // Sine of amplitude 0.5 has RMS = 0.5 / sqrt(2) ≈ 0.354, → ~ -9.0 dBFS.
        let n = 1_600; // exactly one cycle at f=10 Hz, sr=16k — clean RMS.
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                0.5 * (2.0 * std::f32::consts::PI * 10.0 * t).sin()
            })
            .collect();
        let db = rms_dbfs(&samples);
        assert!((db + 9.03).abs() < 0.3, "got {db}, expected ~-9.03");
    }

    #[test]
    fn quiet_signal_well_below_minus_45() {
        // A very quiet noise floor (~-60 dBFS) should be below the filter
        // threshold.
        let q = 0.001_f32;
        let samples = vec![q; 1024];
        let db = rms_dbfs(&samples);
        assert!(db < -45.0, "got {db}, expected < -45");
    }
}
