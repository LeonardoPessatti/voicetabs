//! Library surface for the STT worker. Modules here are re-exported so
//! integration tests under `stt_worker/tests/` can import them without going
//! through the binary.

pub mod framing;
pub mod protocol;
pub mod whisper;
