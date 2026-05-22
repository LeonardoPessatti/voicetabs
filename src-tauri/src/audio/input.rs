use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamConfig};
use crossbeam_channel::{bounded, Receiver, Sender};

/// Target audio configuration. We always request mono 16 kHz f32.
/// WASAPI shared mode handles the device-rate → 16 kHz resampling for us.
#[derive(Debug, Clone, Copy)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self { sample_rate: 16_000, channels: 1 }
    }
}

/// Holds the cpal Stream (which must NOT be dropped while we want audio) and
/// the receiver side of the sample channel. The Stream is `!Send` — keep this
/// struct on the thread that built it.
pub struct InputStreamHandle {
    _stream: Stream,
    pub frames: Receiver<Vec<f32>>,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no default input device available")]
    NoDevice,
    #[error("cpal error: {0}")]
    Cpal(String),
}

/// Build a 16 kHz mono f32 input stream from the system default input device.
///
/// The returned handle owns the `Stream`. As long as the handle is alive and
/// `_stream` is not dropped, audio frames are pushed into `frames`. Dropping
/// the handle stops capture cleanly.
///
/// `channel_capacity` is the upper bound on queued frames; if the consumer
/// stalls, oldest frames are dropped (callback uses `try_send`).
pub fn spawn_input_stream(
    cfg: AudioConfig,
    channel_capacity: usize,
) -> Result<InputStreamHandle, AudioError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(AudioError::NoDevice)?;

    let stream_cfg = StreamConfig {
        channels: cfg.channels,
        sample_rate: SampleRate(cfg.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let (tx, rx) = bounded::<Vec<f32>>(channel_capacity);

    // We always request f32 samples. cpal converts from the device's native
    // format. If conversion fails for some exotic device, `build_input_stream`
    // returns an error and we surface it.
    let supported_format = SampleFormat::F32;

    let err_fn = |e| tracing::warn!("cpal input stream error: {e}");

    let stream = match supported_format {
        SampleFormat::F32 => device
            .build_input_stream(
                &stream_cfg,
                {
                    let tx: Sender<Vec<f32>> = tx;
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        // `data` is borrowed; we must copy before sending.
                        // bounded::try_send drops the frame if the queue is
                        // full — preferable to blocking the audio thread.
                        let frame = data.to_vec();
                        let _ = tx.try_send(frame);
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| AudioError::Cpal(e.to_string()))?,
        _ => return Err(AudioError::Cpal(format!("unsupported sample format: {supported_format:?}"))),
    };

    stream
        .play()
        .map_err(|e| AudioError::Cpal(e.to_string()))?;

    Ok(InputStreamHandle { _stream: stream, frames: rx })
}
