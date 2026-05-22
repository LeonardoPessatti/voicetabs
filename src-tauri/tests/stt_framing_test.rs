//! Smoke tests for the main-process side of the framing protocol. Mirrors the
//! worker-side tests in stt_worker/src/framing.rs.

use std::io::Cursor;

use voicetabs_lib::stt::framing::{
    read_frame, read_frame_async, write_frame, write_frame_async, FrameError,
};

#[test]
fn sync_round_trip() {
    let mut buf = Vec::new();
    write_frame(&mut buf, b"hello").unwrap();
    let mut cur = Cursor::new(buf);
    let got = read_frame(&mut cur).unwrap().unwrap();
    assert_eq!(got, b"hello");
}

#[test]
fn sync_eof_is_none() {
    let mut cur = Cursor::new(Vec::<u8>::new());
    assert!(read_frame(&mut cur).unwrap().is_none());
}

#[test]
fn sync_rejects_oversize_prefix() {
    let too_big = 257u32 * 1024 * 1024;
    let mut buf = Vec::new();
    buf.extend_from_slice(&too_big.to_le_bytes());
    let mut cur = Cursor::new(buf);
    match read_frame(&mut cur) {
        Err(FrameError::TooLarge(_)) => {}
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

#[tokio::test]
async fn async_round_trip() {
    use tokio::io::AsyncWriteExt;
    let (mut client, mut server) = tokio::io::duplex(64);
    // Spawn a writer task that writes one frame then closes the write half.
    let writer = tokio::spawn(async move {
        write_frame_async(&mut client, b"async-hello").await.unwrap();
        client.shutdown().await.unwrap();
    });
    let got = read_frame_async(&mut server).await.unwrap().unwrap();
    writer.await.unwrap();
    assert_eq!(got, b"async-hello");
}
