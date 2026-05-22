use voicetabs_lib::vad::{VadModel, CHUNK_SAMPLES};

#[test]
fn model_loads_and_predicts_silence_low() {
    let mut m = VadModel::new().expect("should build Silero model");
    let silence = vec![0.0_f32; CHUNK_SAMPLES];
    let p = m.predict(&silence).expect("predict ok");
    assert!((0.0..=1.0).contains(&p), "prob out of range: {p}");
    // Silence should be confidently classified as non-speech.
    assert!(p < 0.3, "silence should score low; got {p}");
}

#[test]
fn predict_rejects_wrong_chunk_size() {
    let mut m = VadModel::new().expect("should build Silero model");
    let too_short = vec![0.0_f32; 100];
    assert!(m.predict(&too_short).is_err());
}
