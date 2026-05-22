//! Length-prefixed framing for the STT worker IPC.
//!
//! Wire format: `<u32 little-endian length><payload>` where payload is either
//! UTF-8 JSON or raw `f32` PCM. The two cases are disambiguated by context
//! (the worker alternates between "expecting JSON request" and "expecting PCM
//! payload" based on the request header it just parsed).

use std::io::{Read, Result as IoResult, Write};

/// Hard upper bound to refuse pathological frames. 256 MiB covers a 30 s
/// utterance at 16 kHz f32 (~1.92 MiB) with comfortable headroom.
pub const MAX_FRAME_BYTES: u32 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame length {0} exceeds MAX_FRAME_BYTES")]
    TooLarge(u32),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Write one frame: 4-byte LE length prefix + payload bytes.
pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> Result<(), FrameError> {
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

/// Read one frame's length prefix. Returns `None` on clean EOF (zero bytes
/// before the prefix). Returns an error on partial read.
pub fn read_frame_len<R: Read>(r: &mut R) -> Result<Option<u32>, FrameError> {
    let mut buf = [0u8; 4];
    let mut read = 0;
    while read < 4 {
        match r.read(&mut buf[read..])? {
            0 if read == 0 => return Ok(None),
            0 => {
                return Err(FrameError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "EOF mid-length-prefix",
                )))
            }
            n => read += n,
        }
    }
    let len = u32::from_le_bytes(buf);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    Ok(Some(len))
}

/// Read exactly `len` bytes (after the prefix). Used after a successful
/// `read_frame_len` call.
pub fn read_frame_body<R: Read>(r: &mut R, len: u32) -> IoResult<Vec<u8>> {
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Convenience: read one full frame (prefix + body). Returns `None` on clean
/// EOF before the prefix.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    match read_frame_len(r)? {
        None => Ok(None),
        Some(len) => Ok(Some(read_frame_body(r, len)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_small_payload() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        // 4-byte prefix + 5-byte body
        assert_eq!(buf.len(), 9);
        assert_eq!(&buf[0..4], &5u32.to_le_bytes());
        assert_eq!(&buf[4..], b"hello");

        let mut cur = Cursor::new(buf);
        let got = read_frame(&mut cur).unwrap().unwrap();
        assert_eq!(&got, b"hello");
    }

    #[test]
    fn round_trip_empty_payload() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"").unwrap();
        assert_eq!(buf, &[0u8, 0, 0, 0]);

        let mut cur = Cursor::new(buf);
        let got = read_frame(&mut cur).unwrap().unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn read_frame_returns_none_on_clean_eof() {
        let mut cur = Cursor::new(Vec::<u8>::new());
        let got = read_frame(&mut cur).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn read_frame_errors_on_partial_prefix() {
        let buf = vec![1u8, 0, 0]; // 3 bytes — incomplete prefix
        let mut cur = Cursor::new(buf);
        let err = read_frame(&mut cur).unwrap_err();
        assert!(matches!(err, FrameError::Io(_)));
    }

    #[test]
    fn read_frame_errors_when_body_truncated() {
        let mut buf = Vec::new();
        // Prefix says 5 bytes, only 2 follow.
        buf.extend_from_slice(&5u32.to_le_bytes());
        buf.extend_from_slice(b"hi");
        let mut cur = Cursor::new(buf);
        let err = read_frame(&mut cur).unwrap_err();
        assert!(matches!(err, FrameError::Io(_)));
    }

    #[test]
    fn write_frame_rejects_oversize() {
        // Simulate write of a payload larger than MAX_FRAME_BYTES.
        // We can't actually allocate 256 MiB in a unit test, so we test the
        // bound by hand-poking the limit via a custom slice of length MAX+1
        // — instead, just verify the constant has the expected value and the
        // type system would refuse u32::MAX + 1. The hard-coded check is in
        // read_frame_len; we test that path instead.
        let mut buf = Vec::new();
        let too_big = MAX_FRAME_BYTES + 1;
        buf.extend_from_slice(&too_big.to_le_bytes());
        let mut cur = Cursor::new(buf);
        let err = read_frame_len(&mut cur).unwrap_err();
        assert!(matches!(err, FrameError::TooLarge(_)));
    }

    #[test]
    fn round_trip_pcm_like_payload() {
        // Verify a non-trivial binary payload (e.g. f32-encoded) survives.
        let mut payload = Vec::with_capacity(4 * 16);
        for i in 0..16i32 {
            payload.extend_from_slice(&(i as f32).to_le_bytes());
        }
        let mut buf = Vec::new();
        write_frame(&mut buf, &payload).unwrap();
        let mut cur = Cursor::new(buf);
        let got = read_frame(&mut cur).unwrap().unwrap();
        assert_eq!(got, payload);
    }
}
