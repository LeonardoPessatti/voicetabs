//! Thin wrapper around `whisper-rs`. CPU build only — Phase 7 will introduce
//! a cloud OpenAI path in the main process, not via this subprocess.
//!
//! Adapted for `whisper-rs` 0.16. The plan was authored against 0.13, where
//! per-segment confidence had dedicated getters (`full_get_segment_avg_logprob`,
//! `full_get_segment_no_speech_prob`). In 0.16:
//!   - `state.get_segment(i) -> Option<WhisperSegment>` replaces the
//!     freestanding `full_get_segment_*` family.
//!   - `WhisperSegment::no_speech_probability() -> f32` returns the no-speech
//!     probability directly (no `Result`).
//!   - There is no `avg_logprob` getter at all. We approximate it by averaging
//!     `ln(token_probability)` across all tokens of every segment, which
//!     matches whisper.cpp's own definition (mean per-token log-probability).
//!     `token_probability` is in (0, 1]; we clamp to a small epsilon to keep
//!     `ln` finite when whisper.cpp emits a 0.0.
//!
//! The transcribe function's *shape* (text + avg_logprob + no_speech_prob +
//! duration_ms) is identical to the plan; only the internal getters changed.

use std::path::Path;
use std::time::Instant;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error("whisper context init failed: {0}")]
    Init(String),
    #[error("whisper state init failed: {0}")]
    State(String),
    #[error("whisper inference failed: {0}")]
    Inference(String),
}

/// The transcription result returned by `Engine::transcribe`.
///
/// `text` is the concatenation of segment texts. `avg_logprob` is the mean of
/// `ln(token_probability)` across every token of every segment (this matches
/// whisper.cpp's own definition of segment avg log-prob, generalized to the
/// whole utterance). `no_speech_prob` is the mean of per-segment
/// `no_speech_probability` values.
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub duration_ms: u64,
}

pub struct Engine {
    ctx: WhisperContext,
    model_id: String,
    n_threads: i32,
}

impl Engine {
    /// `model_path` is the absolute path to the `ggml-*.bin` file. `n_threads`
    /// is the number of threads whisper.cpp uses for inference.
    pub fn load(model_path: &Path, n_threads: i32) -> Result<Self, WhisperError> {
        let cparams = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(model_path, cparams)
            .map_err(|e| WhisperError::Init(format!("{e:?}")))?;
        let model_id = model_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Ok(Self {
            ctx,
            model_id,
            n_threads,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn transcribe(
        &mut self,
        samples: &[f32],
        language: &str,
        initial_prompt: &str,
    ) -> Result<TranscriptionResult, WhisperError> {
        let started = Instant::now();
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| WhisperError::State(format!("{e:?}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.n_threads);
        params.set_translate(false);
        params.set_language(Some(language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if !initial_prompt.is_empty() {
            params.set_initial_prompt(initial_prompt);
        }

        state
            .full(params, samples)
            .map_err(|e| WhisperError::Inference(format!("{e:?}")))?;

        // In 0.16 `full_n_segments` returns `c_int` directly (no Result).
        let n_segments = state.full_n_segments();

        let mut text = String::new();
        let mut logprob_sum = 0.0_f64;
        let mut logprob_n: u64 = 0;
        let mut no_speech_prob_sum = 0.0_f64;
        let mut no_speech_n: u64 = 0;

        // Small epsilon so that ln(0.0) doesn't blow up to -inf if whisper.cpp
        // ever reports a token probability of exactly 0.0. ln(1e-10) ≈ -23,
        // which is well outside any realistic logprob threshold but finite.
        const EPS: f32 = 1.0e-10;

        for i in 0..n_segments {
            let segment = match state.get_segment(i) {
                Some(s) => s,
                None => continue,
            };

            // Text. Prefer lossy decoding so a stray invalid byte from a model
            // hiccup doesn't fail the whole utterance.
            let seg_text = segment
                .to_str_lossy()
                .map_err(|e| WhisperError::Inference(format!("{e:?}")))?;
            text.push_str(&seg_text);

            // No-speech probability is a direct f32 in 0.16.
            no_speech_prob_sum += segment.no_speech_probability() as f64;
            no_speech_n += 1;

            // avg_logprob: average ln(token_prob) across this segment's tokens.
            for t in 0..segment.n_tokens() {
                if let Some(token) = segment.get_token(t) {
                    let p = token.token_probability().max(EPS);
                    logprob_sum += (p as f64).ln();
                    logprob_n += 1;
                }
            }
        }

        let avg_logprob = if logprob_n > 0 {
            (logprob_sum / logprob_n as f64) as f32
        } else {
            // No tokens produced (silence / empty result). Pick a sentinel that
            // downstream confidence gates will treat as "low confidence".
            -1.0
        };
        let no_speech_prob = if no_speech_n > 0 {
            (no_speech_prob_sum / no_speech_n as f64) as f32
        } else {
            0.0
        };
        let duration_ms = started.elapsed().as_millis() as u64;

        Ok(TranscriptionResult {
            text: text.trim().to_string(),
            avg_logprob,
            no_speech_prob,
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke-test that the wrapper can load the bundled model and run a
    /// transcription against a silent buffer. Ignored by default because the
    /// model is ~600 MB on disk and loading it takes 5–10 s. Run with:
    ///
    /// ```text
    /// cargo test -p stt_worker --lib whisper -- --ignored
    /// ```
    #[test]
    #[ignore]
    fn engine_loads_model_and_transcribes_silence() {
        // Resolve the model relative to the workspace root so the test works
        // regardless of where `cargo test` is invoked from.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let model_path = std::path::PathBuf::from(manifest_dir)
            .parent()
            .expect("workspace root")
            .join("src-tauri")
            .join("resources")
            .join("ggml-large-v3-turbo-q5_0.bin");
        assert!(
            model_path.exists(),
            "model file not found at {}",
            model_path.display()
        );

        let mut engine = Engine::load(&model_path, 2).expect("engine load");
        assert_eq!(engine.model_id(), "ggml-large-v3-turbo-q5_0");

        // 1 second of silence at 16 kHz. Whisper should return an empty/near
        // empty transcription with a high no_speech_prob.
        let samples = vec![0.0_f32; 16_000];
        let result = engine
            .transcribe(&samples, "en", "")
            .expect("transcribe silence");

        // Confidence gates don't care about exact text here; just verify the
        // shape of the result.
        assert!(result.duration_ms > 0, "duration should be measured");
        assert!(
            result.no_speech_prob >= 0.0 && result.no_speech_prob <= 1.0,
            "no_speech_prob must be a probability, got {}",
            result.no_speech_prob
        );
        // avg_logprob is in (-inf, 0]; for silence it'll either be the -1.0
        // sentinel (no tokens emitted) or some negative value.
        assert!(
            result.avg_logprob <= 0.0,
            "avg_logprob should be non-positive, got {}",
            result.avg_logprob
        );
    }
}
