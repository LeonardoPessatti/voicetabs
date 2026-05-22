//! Length-prefixed framing helpers on the main-process side. Mirrors
//! `stt_worker/src/framing.rs`. See spec §7.4.

use std::io::{Read, Result as IoResult, Write};

pub const MAX_FRAME_BYTES: u32 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame length {0} exceeds MAX_FRAME_BYTES")]
    TooLarge(u32),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

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

pub fn read_frame_body<R: Read>(r: &mut R, len: u32) -> IoResult<Vec<u8>> {
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    match read_frame_len(r)? {
        None => Ok(None),
        Some(len) => Ok(Some(read_frame_body(r, len)?)),
    }
}

/// Tokio-async write of one frame. Used by `SttClient` to write to the child's
/// stdin without blocking the runtime.
pub async fn write_frame_async<W>(w: &mut W, payload: &[u8]) -> Result<(), FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes()).await?;
    w.write_all(payload).await?;
    w.flush().await?;
    Ok(())
}

/// Tokio-async read of one full frame. Returns `Ok(None)` on clean EOF.
pub async fn read_frame_async<R>(r: &mut R) -> Result<Option<Vec<u8>>, FrameError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4];
    match r.read_exact(&mut buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(FrameError::Io(e)),
    }
    let len = u32::from_le_bytes(buf);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(Some(body))
}
