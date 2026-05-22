use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{bounded, Receiver};

/// Target audio configuration for downstream consumers (VAD, WAV writer).
/// We always RESAMPLE to mono 16 kHz f32 after capture. The raw cpal stream
/// is opened at whatever the device supports — see `InputStreamHandle`.
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
///
/// `device_sample_rate` and `device_channels` are the NATIVE values the device
/// is delivering. Frames in `frames` are interleaved f32 in this native shape.
/// Consumers (see `audio::resample::InputAdapter`) downmix + resample to 16 kHz
/// mono before further processing.
pub struct InputStreamHandle {
    _stream: Stream,
    pub frames: Receiver<Vec<f32>>,
    pub device_sample_rate: u32,
    pub device_channels: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no default input device available")]
    NoDevice,
    #[error("cpal error: {0}")]
    Cpal(String),
    #[error("unsupported sample format: {0:?}")]
    UnsupportedFormat(SampleFormat),
}

/// Open the system default input device at its native configuration. The
/// returned handle reports the device's native sample rate + channel count so
/// the caller can downmix + resample to whatever it needs.
///
/// `_cfg` is informational only — kept in the signature for API stability but
/// no longer used to constrain the cpal config (which would fail on many
/// devices that don't expose 16 kHz mono natively).
pub fn spawn_input_stream(
    _cfg: AudioConfig,
    channel_capacity: usize,
) -> Result<InputStreamHandle, AudioError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(AudioError::NoDevice)?;

    let supported = device
        .default_input_config()
        .map_err(|e| AudioError::Cpal(format!("default_input_config: {e}")))?;

    let device_sample_rate = supported.sample_rate().0;
    let device_channels = supported.channels();
    let sample_format = supported.sample_format();
    let stream_cfg: StreamConfig = supported.into();

    tracing::info!(
        sample_rate = device_sample_rate,
        channels = device_channels,
        format = ?sample_format,
        "opening cpal input stream at device-native config"
    );

    let (tx, rx) = bounded::<Vec<f32>>(channel_capacity);
    let err_fn = |e| tracing::warn!("cpal input stream error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => {
            let tx = tx.clone();
            device
                .build_input_stream(
                    &stream_cfg,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let _ = tx.try_send(data.to_vec());
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::Cpal(e.to_string()))?
        }
        SampleFormat::I16 => {
            let tx = tx.clone();
            device
                .build_input_stream(
                    &stream_cfg,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let frame: Vec<f32> = data
                            .iter()
                            .map(|&s| s as f32 / i16::MAX as f32)
                            .collect();
                        let _ = tx.try_send(frame);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::Cpal(e.to_string()))?
        }
        SampleFormat::U16 => {
            let tx = tx.clone();
            device
                .build_input_stream(
                    &stream_cfg,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let frame: Vec<f32> = data
                            .iter()
                            .map(|&s| (s as f32 - 32_768.0) / 32_768.0)
                            .collect();
                        let _ = tx.try_send(frame);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::Cpal(e.to_string()))?
        }
        other => return Err(AudioError::UnsupportedFormat(other)),
    };

    stream.play().map_err(|e| AudioError::Cpal(e.to_string()))?;

    Ok(InputStreamHandle {
        _stream: stream,
        frames: rx,
        device_sample_rate,
        device_channels,
    })
}
