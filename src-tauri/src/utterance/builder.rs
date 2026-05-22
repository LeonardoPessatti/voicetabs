use std::path::PathBuf;

use crate::utterance::{preroll::PreRoll, wav::write_pcm16_wav};
use crate::vad::VadEvent;

/// Configuration for the builder.
#[derive(Debug, Clone, Copy)]
pub struct UtteranceConfig {
    pub sample_rate: u32,
    pub pre_roll_ms: u32,
    pub max_utterance_ms: u32,
}

impl Default for UtteranceConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            pre_roll_ms: 500,
            max_utterance_ms: 30_000,
        }
    }
}

/// The currently in-progress utterance, if any.
struct Current {
    started_at_ms: u64,
    samples: Vec<f32>,
}

/// Builds utterance WAV files from a stream of audio frames + VAD events.
///
/// The builder is a pure data-flow component: it does not own the audio
/// thread, nor any I/O loop. The caller drives it via `push_frame` and
/// `on_vad_event`. When `on_vad_event` returns `Some(path)` or `push_frame`
/// triggers a max-cap finalization that returns `Some(path)`, a WAV file has
/// been written.
pub struct UtteranceBuilder {
    cfg: UtteranceConfig,
    pre_roll: PreRoll,
    current: Option<Current>,
    output_dir: PathBuf,
    max_samples: usize,
}

impl UtteranceBuilder {
    pub fn new(cfg: UtteranceConfig, output_dir: PathBuf) -> Self {
        let pre_roll_samples =
            (cfg.sample_rate as usize * cfg.pre_roll_ms as usize) / 1000;
        let max_samples =
            (cfg.sample_rate as usize * cfg.max_utterance_ms as usize) / 1000;
        Self {
            cfg,
            pre_roll: PreRoll::new(pre_roll_samples),
            current: None,
            output_dir,
            max_samples,
        }
    }

    /// Append audio samples. If we're currently recording, they go straight
    /// into the utterance and the max-duration cap is checked. Otherwise they
    /// go into the pre-roll ring.
    ///
    /// Returns `Some(path)` if the max-duration cap was hit and a WAV was
    /// emitted as a result.
    pub fn push_frame(&mut self, samples: &[f32]) -> Option<PathBuf> {
        if let Some(current) = &mut self.current {
            current.samples.extend_from_slice(samples);
            if current.samples.len() >= self.max_samples {
                return self.finalize();
            }
            None
        } else {
            self.pre_roll.push(samples);
            None
        }
    }

    /// Handle a VAD edge event. Returns `Some(path)` if a WAV was emitted as
    /// a result (only on FallingEdge or max-cap finalization).
    pub fn on_vad_event(&mut self, event: VadEvent) -> Option<PathBuf> {
        match event {
            VadEvent::RisingEdge { timestamp_ms } => {
                let mut samples = self.pre_roll.drain();
                // Reserve a moderate chunk so common short utterances don't
                // reallocate the Vec repeatedly.
                samples.reserve(self.cfg.sample_rate as usize * 2);
                self.current = Some(Current {
                    started_at_ms: timestamp_ms,
                    samples,
                });
                None
            }
            VadEvent::FallingEdge { .. } => self.finalize(),
        }
    }

    fn finalize(&mut self) -> Option<PathBuf> {
        let current = self.current.take()?;
        if current.samples.is_empty() {
            return None;
        }
        let path = self
            .output_dir
            .join(format!("{}.wav", current.started_at_ms));
        match write_pcm16_wav(&path, self.cfg.sample_rate, &current.samples) {
            Ok(()) => Some(path),
            Err(e) => {
                tracing::error!("failed to write utterance WAV {path:?}: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;
    use tempfile::tempdir;

    fn ones(n: usize) -> Vec<f32> {
        vec![0.5_f32; n]
    }

    fn cfg() -> UtteranceConfig {
        UtteranceConfig {
            sample_rate: 16_000,
            pre_roll_ms: 500,
            max_utterance_ms: 30_000,
        }
    }

    #[test]
    fn idle_frames_go_into_pre_roll() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());
        // Push 320 samples (20 ms). Idle → into pre-roll.
        let path = b.push_frame(&ones(320));
        assert!(path.is_none());
        // No file yet.
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn rising_edge_drains_pre_roll_into_current() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());
        b.push_frame(&ones(1_000));
        let result = b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 42 });
        assert!(result.is_none(), "rising edge does not write a file");
        // After rising, more frames go into the current utterance.
        b.push_frame(&ones(500));
        let path = b
            .on_vad_event(VadEvent::FallingEdge { timestamp_ms: 100 })
            .expect("falling edge writes WAV");
        // The filename uses the rising-edge timestamp.
        assert!(path.to_string_lossy().ends_with("42.wav"));
        // The WAV contains pre-roll (1000) + recorded (500) = 1500 samples.
        let reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.duration() as usize, 1_500);
    }

    #[test]
    fn falling_edge_without_rising_is_a_noop() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());
        let path = b.on_vad_event(VadEvent::FallingEdge { timestamp_ms: 0 });
        assert!(path.is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn max_duration_cap_force_finalizes() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(
            UtteranceConfig {
                sample_rate: 16_000,
                pre_roll_ms: 0,
                max_utterance_ms: 1_000, // tiny cap so we hit it fast
            },
            dir.path().to_path_buf(),
        );
        b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 7 });
        // 16 000 samples per second × 1 s cap = 16 000 samples; one frame puts
        // us over.
        let result = b.push_frame(&ones(20_000));
        let path = result.expect("max cap should finalize");
        assert!(path.to_string_lossy().ends_with("7.wav"));
    }

    #[test]
    fn two_utterances_produce_two_files() {
        let dir = tempdir().unwrap();
        let mut b = UtteranceBuilder::new(cfg(), dir.path().to_path_buf());

        b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 1 });
        b.push_frame(&ones(800));
        let first = b
            .on_vad_event(VadEvent::FallingEdge { timestamp_ms: 50 })
            .expect("first WAV");
        assert!(first.to_string_lossy().ends_with("1.wav"));

        b.on_vad_event(VadEvent::RisingEdge { timestamp_ms: 100 });
        b.push_frame(&ones(800));
        let second = b
            .on_vad_event(VadEvent::FallingEdge { timestamp_ms: 200 })
            .expect("second WAV");
        assert!(second.to_string_lossy().ends_with("100.wav"));

        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(files.len(), 2);
    }
}
