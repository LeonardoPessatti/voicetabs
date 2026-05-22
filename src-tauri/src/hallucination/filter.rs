//! Decide whether to keep or drop a transcription.
//!
//! Pure function, side-effect free. The caller passes the post-STT text and
//! the metadata produced by both the worker (avg_logprob, no_speech_prob) and
//! the audio pipeline (rms_dbfs). The function returns `Decision::Keep` or
//! `Decision::Drop(reason)`; the caller is responsible for logging the reason
//! and discarding the result.
//!
//! Thresholds and the blocklist follow spec §5.2 step 8 + §7.5.

/// Substrings (lowercase, ASCII-folded) that trigger a drop. Match is
/// substring (case-insensitive). The list is intentionally short; extend
/// here, not via settings, so it ships with the binary.
pub const BLOCKLIST: &[&str] = &[
    "obrigado por assistir",
    "legendas pela comunidade amara.org",
    "thanks for watching",
    "thank you for watching",
    "subtitles by",
    "subscribe to my channel",
];

/// Tunables. Kept on the struct rather than as module constants so a future
/// settings-driven version is a one-line change.
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub no_speech_max: f32,    // drop if no_speech_prob > this
    pub avg_logprob_min: f32,  // drop if avg_logprob < this
    pub rms_dbfs_min: f32,     // drop if rms_dbfs < this
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            no_speech_max: 0.6,
            avg_logprob_min: -1.0,
            rms_dbfs_min: -45.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Input<'a> {
    pub text: &'a str,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub rms_dbfs: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Drop(DropReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropReason {
    Empty,
    Blocklist(String),     // the matched substring
    NoSpeech,              // no_speech_prob > threshold
    LowLogprob,            // avg_logprob < threshold
    LowEnergy,             // rms_dbfs < threshold
}

/// Apply the filter. Order matters only for the logged reason: we check the
/// cheapest predicates first so the most informative reason wins.
pub fn evaluate(input: &Input, thresholds: &Thresholds) -> Decision {
    let normalized = normalize(input.text);
    if normalized.is_empty() {
        return Decision::Drop(DropReason::Empty);
    }
    for entry in BLOCKLIST {
        if normalized.contains(entry) {
            return Decision::Drop(DropReason::Blocklist((*entry).to_string()));
        }
    }
    if input.no_speech_prob > thresholds.no_speech_max {
        return Decision::Drop(DropReason::NoSpeech);
    }
    if input.avg_logprob < thresholds.avg_logprob_min {
        return Decision::Drop(DropReason::LowLogprob);
    }
    if input.rms_dbfs < thresholds.rms_dbfs_min {
        return Decision::Drop(DropReason::LowEnergy);
    }
    Decision::Keep
}

/// Lowercase + collapse internal whitespace + ASCII-fold a handful of
/// accented characters that appear in the blocklist. Pure ASCII compare so
/// the test fixtures stay readable.
fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_was_space = true; // trim leading whitespace
    for c in lowered.chars() {
        let folded = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        };
        if folded.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(folded);
            last_was_space = false;
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Input<'static> {
        Input {
            text: "Olá, como vai você?",
            avg_logprob: -0.3,
            no_speech_prob: 0.02,
            rms_dbfs: -20.0,
        }
    }

    #[test]
    fn keep_well_formed_speech() {
        assert_eq!(evaluate(&good(), &Thresholds::default()), Decision::Keep);
    }

    #[test]
    fn drop_empty_text() {
        let mut i = good();
        i.text = "";
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::Empty)));
    }

    #[test]
    fn drop_whitespace_only_text() {
        let mut i = good();
        i.text = "   \n\t  ";
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::Empty)));
    }

    #[test]
    fn drop_blocklisted_substring_case_insensitive() {
        let mut i = good();
        i.text = "  OBRIGADO POR ASSISTIR! até a próxima.";
        match evaluate(&i, &Thresholds::default()) {
            Decision::Drop(DropReason::Blocklist(s)) => assert_eq!(s, "obrigado por assistir"),
            other => panic!("expected Blocklist drop, got {other:?}"),
        }
    }

    #[test]
    fn drop_blocklisted_with_accents() {
        // The text has accents; the blocklist entry does not. Our normalizer
        // folds before comparing, so this matches.
        let mut i = good();
        i.text = "Obrigádo pôr assistir";
        assert!(matches!(
            evaluate(&i, &Thresholds::default()),
            Decision::Drop(DropReason::Blocklist(_))
        ));
    }

    #[test]
    fn drop_when_no_speech_prob_too_high() {
        let mut i = good();
        i.no_speech_prob = 0.85;
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::NoSpeech)));
    }

    #[test]
    fn drop_when_avg_logprob_too_low() {
        let mut i = good();
        i.avg_logprob = -1.5;
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::LowLogprob)));
    }

    #[test]
    fn drop_when_rms_too_low() {
        let mut i = good();
        i.rms_dbfs = -50.0;
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::LowEnergy)));
    }

    #[test]
    fn empty_takes_priority_over_other_failures() {
        let i = Input {
            text: "",
            avg_logprob: -5.0,
            no_speech_prob: 0.99,
            rms_dbfs: -80.0,
        };
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::Empty)));
    }
}
