//! Build the Whisper `initial_prompt` string from a list of vocab terms and a
//! language code. Pure function, side-effect free, easy to unit-test.

/// Construct an `initial_prompt` for the given vocab terms.
///
/// - `terms`: user-entered vocabulary. Empty / whitespace-only entries are
///   skipped; surrounding whitespace is trimmed; terms are joined with
///   ", " in the order given.
/// - `language`: `"pt"` produces `"Termos: t1, t2, t3."`; anything else
///   produces `"Terms: t1, t2, t3."`. (We currently only ship `"pt"` and
///   `"en"`; the spec leaves room to grow.)
///
/// Returns an empty string if no usable terms remain — the STT request will
/// then send an empty `initial_prompt`, which Whisper accepts.
pub fn build_initial_prompt(terms: &[String], language: &str) -> String {
    let cleaned: Vec<&str> = terms
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if cleaned.is_empty() {
        return String::new();
    }
    let joined = cleaned.join(", ");
    let head = if language == "pt" { "Termos" } else { "Terms" };
    format!("{head}: {joined}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_terms_returns_empty_prompt() {
        assert_eq!(build_initial_prompt(&[], "pt"), "");
        assert_eq!(build_initial_prompt(&[], "en"), "");
    }

    #[test]
    fn whitespace_only_terms_return_empty_prompt() {
        let terms = vec!["".into(), "   ".into(), "\t\n".into()];
        assert_eq!(build_initial_prompt(&terms, "pt"), "");
    }

    #[test]
    fn pt_prefix_used_for_portuguese() {
        let terms = vec!["VoiceTabs".into(), "whisper.cpp".into()];
        assert_eq!(
            build_initial_prompt(&terms, "pt"),
            "Termos: VoiceTabs, whisper.cpp."
        );
    }

    #[test]
    fn en_prefix_used_for_english() {
        let terms = vec!["VoiceTabs".into()];
        assert_eq!(
            build_initial_prompt(&terms, "en"),
            "Terms: VoiceTabs."
        );
    }

    #[test]
    fn terms_are_trimmed_individually() {
        let terms = vec!["  pádua  ".into(), "leo".into()];
        assert_eq!(
            build_initial_prompt(&terms, "pt"),
            "Termos: pádua, leo."
        );
    }

    #[test]
    fn order_is_preserved() {
        let terms = vec!["c".into(), "a".into(), "b".into()];
        assert_eq!(
            build_initial_prompt(&terms, "en"),
            "Terms: c, a, b."
        );
    }
}
