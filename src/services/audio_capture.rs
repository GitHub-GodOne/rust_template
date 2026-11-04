//! 远程音频: what this machine's speakers play, captured for listening in
//! the browser.
//!
//! Windows records the default playback device through WASAPI loopback
//! (`cpal`), Linux the default `PulseAudio` / `PipeWire` monitor through
//! `parec`; other systems are not supported. Every capture delivers 16-bit
//! little-endian PCM in [`Quality`]'s rate and channel count, in chunks of
//! [`CHUNK_MS`]. Digital silence is dropped, so a machine playing nothing
//! costs no traffic.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::sync::mpsc;

/// Length of one chunk of audio.
pub const CHUNK_MS: u32 = 40;
/// Chunks waiting for a slow listener before newer ones are dropped.
const QUEUE_CHUNKS: usize = 50;
/// Peak sample below which a chunk counts as silence.
const SILENCE_PEAK: i16 = 4;

/// Output format of a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quality {
    pub key: &'static str,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Quality {
    /// PCM bitrate while something is playing.
    #[must_use]
    pub const fn bitrate_kbps(&self) -> u32 {
        self.sample_rate * self.channels as u32 * 16 / 1000
    }
}

pub const QUALITIES: [Quality; 3] = [
    Quality {
        key: "high",
        sample_rate: 48_000,
        channels: 2,
    },
    Quality {
        key: "standard",
        sample_rate: 24_000,
        channels: 2,
    },
    Quality {
        key: "low",
        sample_rate: 16_000,
        channels: 1,
    },
];

#[must_use]
pub fn quality(key: &str) -> Option<Quality> {
    QUALITIES.iter().copied().find(|quality| quality.key == key)
}

#[derive(Debug)]
pub enum Chunk {
    /// Interleaved samples in the capture's quality.
    Pcm(Vec<i16>),
    /// The capture stopped, e.g. the playback device went away.
    Error(String),
}

/// Whether this machine can capture, and from what.
#[derive(Debug, Clone)]
pub struct Support {
    pub supported: bool,
    /// `wasapi` or `pulseaudio`.
    pub backend: Option<&'static str>,
    pub device: Option<String>,
    pub message: Option<String>,
}

/// A running capture; dropping it stops recording.
pub struct Capture {
    pub chunks: mpsc::Receiver<Chunk>,
    pub device: String,
    stop: Arc<AtomicBool>,
    #[cfg(not(windows))]
    _process: Option<platform::Process>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Probes the capture backend (blocking: it asks the audio system).
#[must_use]
pub fn support() -> Support {
    platform::support()
}

/// Starts recording in `quality`.
///
/// # Errors
///
/// When there is no playback device or the backend cannot record it.
pub async fn start(quality: Quality) -> Result<Capture, String> {
    platform::start(quality).await
}

/// Turns the device's float frames into `quality`'s format: downmixes or
/// duplicates channels, resamples, converts to `i16` and cuts the stream into
/// chunks.
pub struct Converter {
    source_channels: usize,
    channels: usize,
    /// Source frames per output frame.
    step: f64,
    /// Downmixed source frames not consumed yet.
    pending: Vec<f32>,
    /// Position of the next output frame in `pending`, in frames.
    position: f64,
    chunk_samples: usize,
    output: Vec<i16>,
}

impl Converter {
    #[must_use]
    pub fn new(source_rate: u32, source_channels: u16, quality: Quality) -> Self {
        let channels = usize::from(quality.channels.max(1));
        Self {
            source_channels: usize::from(source_channels.max(1)),
            channels,
            step: f64::from(source_rate.max(1)) / f64::from(quality.sample_rate.max(1)),
            pending: Vec::new(),
            position: 0.0,
            chunk_samples: (quality.sample_rate * CHUNK_MS / 1000) as usize * channels,
            output: Vec::new(),
        }
    }

    /// Feeds interleaved source samples; returns the chunks completed.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<i16>> {
        for frame in samples.chunks_exact(self.source_channels) {
            if self.channels == 1 {
                #[allow(clippy::cast_precision_loss)]
                self.pending
                    .push(frame.iter().sum::<f32>() / frame.len() as f32);
            } else {
                let left = frame[0];
                let right = frame.get(1).copied().unwrap_or(left);
                self.pending.extend([left, right]);
            }
        }
        self.resample();
        let mut chunks = Vec::new();
        while self.output.len() >= self.chunk_samples {
            let rest = self.output.split_off(self.chunk_samples);
            let chunk = std::mem::replace(&mut self.output, rest);
            if chunk
                .iter()
                .any(|sample| sample.unsigned_abs() >= SILENCE_PEAK.unsigned_abs())
            {
                chunks.push(chunk);
            }
        }
        chunks
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn resample(&mut self) {
        let channels = self.channels;
        let frames = self.pending.len() / channels;
        loop {
            let start = self.position.floor() as usize;
            // Downsampling averages the frames an output frame covers, which
            // keeps most aliasing out; upsampling interpolates linearly.
            let end = if self.step >= 1.0 {
                (self.position + self.step).floor() as usize
            } else {
                start + 1
            };
            // Frames read: `start..end` when averaging, `start` and the next
            // one when interpolating.
            let needed = if self.step >= 1.0 { end } else { start + 2 };
            if needed > frames {
                break;
            }
            for channel in 0..channels {
                let value = if self.step >= 1.0 {
                    let count = (end - start).max(1);
                    (start..start + count)
                        .map(|frame| self.pending[frame * channels + channel])
                        .sum::<f32>()
                        / count as f32
                } else {
                    let fraction = (self.position - start as f64) as f32;
                    let current = self.pending[start * channels + channel];
                    let next = self.pending[(start + 1) * channels + channel];
                    (next - current).mul_add(fraction, current)
                };
                self.output
                    .push((value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16);
            }
            self.position += self.step;
        }
        let consumed = (self.position.floor() as usize).min(frames);
        self.pending.drain(..consumed * channels);
        self.position -= consumed as f64;
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        sync::{atomic::Ordering, mpsc as std_mpsc, Arc},
        thread,
        time::Duration,
    };

    use cpal::{
        traits::{DeviceTrait, HostTrait, StreamTrait},
        FromSample, SampleFormat, SizedSample,
    };
    use tokio::sync::mpsc;

    use super::{Capture, Chunk, Converter, Quality, Support, QUEUE_CHUNKS};

    pub fn support() -> Support {
        cpal::default_host().default_output_device().map_or_else(
            || Support {
                supported: false,
                backend: Some("wasapi"),
                device: None,
                message: Some(
                    "本机没有可用的播放设备（需要声卡或扬声器，且服务运行在已登录的用户会话中）"
                        .to_string(),
                ),
            },
            |device| Support {
                supported: true,
                backend: Some("wasapi"),
                device: device.name().ok(),
                message: None,
            },
        )
    }

    pub async fn start(quality: Quality) -> Result<Capture, String> {
        let (sender, chunks) = mpsc::channel(QUEUE_CHUNKS);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (ready_sender, ready) = std_mpsc::channel();
        let running = Arc::clone(&stop);
        // A cpal stream must stay on the thread that built it.
        thread::spawn(move || match open(quality, sender) {
            Ok((stream, device)) => {
                let _ = ready_sender.send(Ok(device));
                while !running.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
                drop(stream);
            }
            Err(error) => {
                let _ = ready_sender.send(Err(error));
            }
        });
        let device = tokio::task::spawn_blocking(move || {
            ready
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| Err("音频设备没有响应".to_string()))
        })
        .await
        .map_err(|error| error.to_string())??;
        Ok(Capture {
            chunks,
            device,
            stop,
        })
    }

    fn open(
        quality: Quality,
        sender: mpsc::Sender<Chunk>,
    ) -> Result<(cpal::Stream, String), String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                "本机没有可用的播放设备（需要声卡或扬声器，且服务运行在已登录的用户会话中）"
                    .to_string()
            })?;
        let name = device.name().unwrap_or_default();
        let supported = device
            .default_output_config()
            .map_err(|error| format!("读取播放设备格式失败：{error}"))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        // Building an input stream on an output device records it (loopback).
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, &config, quality, sender),
            SampleFormat::I16 => build::<i16>(&device, &config, quality, sender),
            SampleFormat::U16 => build::<u16>(&device, &config, quality, sender),
            SampleFormat::I32 => build::<i32>(&device, &config, quality, sender),
            other => return Err(format!("不支持的播放设备格式：{other}")),
        }
        .map_err(|error| format!("启动音频采集失败：{error}"))?;
        stream
            .play()
            .map_err(|error| format!("启动音频采集失败：{error}"))?;
        Ok((stream, name))
    }

    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        quality: Quality,
        sender: mpsc::Sender<Chunk>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let mut converter = Converter::new(config.sample_rate.0, config.channels, quality);
        let mut samples = Vec::new();
        let errors = sender.clone();
        device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                samples.clear();
                samples.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
                for chunk in converter.push(&samples) {
                    // A listener that falls behind loses audio, not latency.
                    let _ = sender.try_send(Chunk::Pcm(chunk));
                }
            },
            move |error| {
                let _ = errors.try_send(Chunk::Error(error.to_string()));
            },
            None,
        )
    }
}

#[cfg(not(windows))]
mod platform {
    use std::{process::Stdio, sync::Arc};

    use tokio::{io::AsyncReadExt, process::Command, sync::mpsc};

    use super::{Capture, Chunk, Converter, Quality, Support, QUEUE_CHUNKS};
    use crate::services::child_processes;

    /// The `parec` recording, ended with the capture.
    pub struct Process {
        _child: tokio::process::Child,
        _tracked: child_processes::Tracked,
        reader: tokio::task::JoinHandle<()>,
    }

    impl Drop for Process {
        fn drop(&mut self) {
            self.reader.abort();
        }
    }

    #[cfg(target_os = "linux")]
    pub fn support() -> Support {
        let available = std::process::Command::new("parec")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        Support {
            supported: available,
            backend: Some("pulseaudio"),
            device: available.then(|| "@DEFAULT_MONITOR@".to_string()),
            message: (!available).then(|| {
                "未找到 parec，请安装 PulseAudio 或 PipeWire（pipewire-pulse）".to_string()
            }),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn support() -> Support {
        Support {
            supported: false,
            backend: None,
            device: None,
            message: Some("当前系统不支持远程音频采集".to_string()),
        }
    }

    pub async fn start(quality: Quality) -> Result<Capture, String> {
        let support = support();
        if !support.supported {
            return Err(support.message.unwrap_or_default());
        }
        let mut child = Command::new("parec")
            .args([
                "--raw",
                "--format=s16le",
                &format!("--rate={}", quality.sample_rate),
                &format!("--channels={}", quality.channels),
                "--latency-msec=40",
                "--device=@DEFAULT_MONITOR@",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("启动音频采集失败：{error}"))?;
        let tracked = child_processes::track(&child);
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "启动音频采集失败：没有输出".to_string())?;
        let (sender, chunks) = mpsc::channel(QUEUE_CHUNKS);
        // parec already delivers the target format; the converter only cuts
        // chunks and drops silence.
        let mut converter = Converter::new(quality.sample_rate, quality.channels, quality);
        let reader = tokio::spawn(async move {
            let mut buffer = vec![0_u8; 8192];
            let mut carry: Option<u8> = None;
            loop {
                match stdout.read(&mut buffer).await {
                    Ok(0) | Err(_) => {
                        let _ = sender.try_send(Chunk::Error("音频采集已结束".to_string()));
                        return;
                    }
                    Ok(size) => {
                        let mut bytes: Vec<u8> = carry.take().into_iter().collect();
                        bytes.extend_from_slice(&buffer[..size]);
                        if bytes.len() % 2 == 1 {
                            carry = bytes.pop();
                        }
                        let samples: Vec<f32> = bytes
                            .chunks_exact(2)
                            .map(|pair| {
                                f32::from(i16::from_le_bytes([pair[0], pair[1]]))
                                    / f32::from(i16::MAX)
                            })
                            .collect();
                        for chunk in converter.push(&samples) {
                            let _ = sender.try_send(Chunk::Pcm(chunk));
                        }
                    }
                }
            }
        });
        Ok(Capture {
            chunks,
            device: support.device.unwrap_or_default(),
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            _process: Some(Process {
                _child: child,
                _tracked: tracked,
                reader,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{quality, Converter, Quality, CHUNK_MS};

    const STEREO_48K: Quality = Quality {
        key: "test",
        sample_rate: 48_000,
        channels: 2,
    };

    fn tone(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|frame| {
                #[allow(clippy::cast_precision_loss)]
                let value = (frame as f32 / 10.0).sin() * 0.5;
                std::iter::repeat_n(value, channels)
            })
            .collect()
    }

    #[test]
    fn chunks_have_the_quality_length() {
        let mut converter = Converter::new(48_000, 2, STEREO_48K);
        // One second of a tone gives 25 chunks of 40 ms.
        let chunks = converter.push(&tone(48_000, 2));
        assert_eq!(chunks.len(), 25);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.len() == (48_000 * CHUNK_MS / 1000) as usize * 2));
    }

    #[test]
    fn resampling_and_downmixing_keep_the_duration() {
        let low = quality("low").unwrap();
        let mut converter = Converter::new(44_100, 6, low);
        let mut samples = 0;
        // Fed in uneven blocks, as devices do.
        for block in tone(44_100 * 2, 6).chunks(6 * 441) {
            samples += converter.push(block).iter().map(Vec::len).sum::<usize>();
        }
        // Two seconds of mono 16 kHz, give or take the last partial chunk.
        assert!((31_000..=32_000).contains(&samples), "{samples}");
    }

    #[test]
    fn silence_costs_nothing() {
        let mut converter = Converter::new(48_000, 2, STEREO_48K);
        assert!(converter.push(&vec![0.0; 48_000 * 2]).is_empty());
        assert_eq!(converter.push(&tone(48_000, 2)).len(), 25);
    }

    /// `cargo test --lib captures_the_playback_device -- --ignored --nocapture`
    /// while the machine plays something.
    #[tokio::test]
    #[ignore = "records this machine's speakers; play something meanwhile"]
    async fn captures_the_playback_device() {
        let quality = quality("standard").unwrap();
        let mut capture = super::start(quality).await.unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut chunks = 0;
        while let Ok(Some(chunk)) = tokio::time::timeout_at(deadline, capture.chunks.recv()).await {
            match chunk {
                super::Chunk::Pcm(samples) => {
                    assert_eq!(samples.len(), 960 * 2);
                    chunks += 1;
                }
                super::Chunk::Error(error) => panic!("{error}"),
            }
        }
        println!("captured {chunks} chunks from {}", capture.device);
        assert!(chunks > 0, "nothing was playing");
    }

    #[test]
    fn mono_sources_fill_both_channels() {
        let mut converter = Converter::new(48_000, 1, STEREO_48K);
        let chunk = &converter.push(&tone(1_920, 1))[0];
        assert!(chunk.chunks_exact(2).all(|pair| pair[0] == pair[1]));
    }
}
