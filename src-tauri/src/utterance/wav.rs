use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

/// Write a 16 kHz mono 16-bit PCM WAV from f32 samples in [-1.0, 1.0].
/// Samples outside the range are clipped.
pub fn write_pcm16_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> anyhow::Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)?;
    for &s in samples {
        let clipped = s.clamp(-1.0, 1.0);
        // Symmetric scale so 1.0 → 32767 and -1.0 → -32767. Avoids the
        // off-by-one at the negative extreme that you'd get with 32768.0.
        let v = (clipped * 32767.0) as i16;
        writer.write_sample(v)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;
    use tempfile::tempdir;

    #[test]
    fn round_trip_preserves_sample_rate_and_channel() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let samples: Vec<f32> = (0..16_000).map(|i| (i as f32 / 1000.0).sin()).collect();
        write_pcm16_wav(&path, 16_000, &samples).unwrap();

        let reader = WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);
    }

    #[test]
    fn clips_out_of_range_input() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let samples = vec![2.0_f32, -2.0, 0.5, -0.5];
        write_pcm16_wav(&path, 16_000, &samples).unwrap();

        let mut reader = WavReader::open(&path).unwrap();
        let read_back: Vec<i16> = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(read_back.len(), 4);
        assert_eq!(read_back[0], 32_767); // clipped from 2.0
        assert_eq!(read_back[1], -32_767); // clipped from -2.0
        assert!((read_back[2] - 16_383).abs() <= 1, "got {}", read_back[2]);
        assert!((read_back[3] + 16_383).abs() <= 1, "got {}", read_back[3]);
    }

    #[test]
    fn writes_an_empty_file_for_empty_input() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        write_pcm16_wav(&path, 16_000, &[]).unwrap();
        let reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.duration(), 0);
    }
}
