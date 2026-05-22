use voice_activity_detector::VoiceActivityDetector;

/// Number of f32 samples per VAD prediction at 16 kHz. 512 samples ≈ 32 ms,
/// which is one of the chunk sizes Silero v5 was trained for.
pub const CHUNK_SAMPLES: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum VadModelError {
    #[error("failed to load Silero VAD model: {0}")]
    Build(String),
    #[error("wrong chunk size: expected {expected}, got {got}")]
    ChunkSize { expected: usize, got: usize },
}

/// Thin wrapper around the Silero VAD model. Holds LSTM state internally;
/// every call to `predict` updates the state, so the wrapper is mutable and
/// must not be shared across threads.
pub struct VadModel {
    inner: VoiceActivityDetector,
}

impl VadModel {
    pub fn new() -> Result<Self, VadModelError> {
        let inner = VoiceActivityDetector::builder()
            .sample_rate(16_000_i64)
            .chunk_size(CHUNK_SAMPLES)
            .build()
            .map_err(|e| VadModelError::Build(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Run inference on exactly `CHUNK_SAMPLES` samples. Returns p(speech) in
    /// the range [0.0, 1.0].
    pub fn predict(&mut self, samples: &[f32]) -> Result<f32, VadModelError> {
        if samples.len() != CHUNK_SAMPLES {
            return Err(VadModelError::ChunkSize {
                expected: CHUNK_SAMPLES,
                got: samples.len(),
            });
        }
        Ok(self.inner.predict(samples.iter().copied()))
    }
}
