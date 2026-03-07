use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Audio capture manager — supports cpal (microphone) and HAL Tap (app audio)
pub struct AudioCapture {
    running: Arc<AtomicBool>,
    /// Hold stream thread handle (cpal path)
    stream_thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// Audio chunk sent to processing pipeline
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// PCM samples, f32, mono, 16kHz
    pub samples: Vec<f32>,
    /// Sample rate
    pub sample_rate: u32,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            stream_thread: std::sync::Mutex::new(None),
        }
    }

    /// List available audio input devices
    pub fn list_devices() -> Result<Vec<String>, String> {
        let host = cpal::default_host();
        let mut devices = Vec::new();
        if let Ok(input_devices) = host.input_devices() {
            for device in input_devices {
                if let Ok(name) = device.name() {
                    devices.push(name);
                }
            }
        }
        Ok(devices)
    }

    /// Start capturing audio from the configured source (system audio via cpal).
    pub fn start_with_source(
        &self,
        _source: &str,
        device_name: &str,
        chunk_duration_ms: u32,
        tx: mpsc::UnboundedSender<AudioChunk>,
    ) -> Result<(), String> {
        self.start(device_name, chunk_duration_ms, tx)
    }

    /// Start capturing audio from a cpal input device (system virtual device).
    ///
    /// `cpal::Stream` is `!Send`, so we spawn a dedicated thread that creates
    /// the stream, plays it, and holds it alive until `stop()` is called.
    pub fn start(
        &self,
        device_name: &str,
        chunk_duration_ms: u32,
        tx: mpsc::UnboundedSender<AudioChunk>,
    ) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Audio capture already running".into());
        }

        let device_name_owned = device_name.to_string();
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        // Channel to report initialization result back to caller
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();

        let running_thread = self.running.clone();
        let handle = std::thread::spawn(move || {
            let init_result = build_and_run_stream(
                &device_name_owned,
                chunk_duration_ms,
                tx,
                running_thread.clone(),
            );
            match init_result {
                Ok(stream) => {
                    let _ = init_tx.send(Ok(()));
                    // Hold the stream alive on this thread
                    while running_thread.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    drop(stream);
                }
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                }
            }
        });

        // Wait for the thread to report success or failure
        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                self.running.store(false, Ordering::SeqCst);
                return Err(e);
            }
            Err(_) => {
                self.running.store(false, Ordering::SeqCst);
                return Err("Audio thread failed to start".into());
            }
        }

        if let Ok(mut t) = self.stream_thread.lock() {
            *t = Some(handle);
        }

        Ok(())
    }

    /// Stop capturing
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        // Stop cpal stream thread if active
        if let Ok(mut t) = self.stream_thread.lock() {
            if let Some(handle) = t.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// Build and start a cpal input stream on the current thread.
/// Returns the stream handle (must be kept alive to continue capturing).
fn build_and_run_stream(
    device_name: &str,
    chunk_duration_ms: u32,
    tx: mpsc::UnboundedSender<AudioChunk>,
    running: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    log::info!("[Audio] Using host: {:?}", host.id());

    let device = if device_name.is_empty() {
        let d = host.default_input_device()
            .ok_or_else(|| "No default input device found. On macOS, check that microphone permission is granted in System Settings > Privacy & Security > Microphone.".to_string())?;
        log::info!("[Audio] Using default input device: {:?}", d.name().unwrap_or_default());
        d
    } else {
        let d = host.input_devices()
            .map_err(|e| format!("Failed to enumerate input devices: {}", e))?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
            .ok_or_else(|| format!("Device '{}' not found", device_name))?;
        log::info!("[Audio] Using specified input device: {}", device_name);
        d
    };

    let supported = device.default_input_config()
        .map_err(|e| format!("Failed to get input config for device (check microphone permissions): {}", e))?;
    let sample_format = supported.sample_format();
    let native_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;

    let config = StreamConfig {
        channels: supported.channels(),
        sample_rate: SampleRate(native_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // Shared state for the audio callback
    // Clamp chunk duration to safe range [20, 2000] ms
    let duration_ms = chunk_duration_ms.clamp(20, 2000) as f64;
    let chunk_samples = (16000.0_f64 * duration_ms / 1000.0) as usize;
    let need_resample = native_rate != 16000;
    let resample_ratio = 16000.0 / native_rate as f64;

    let err_fn = |err: cpal::StreamError| {
        log::error!("Audio stream error: {}", err);
    };

    // Common processor: accepts mono f32 samples, resamples, chunks, and sends
    let make_processor = move || {
        let mut buffer: Vec<f32> = Vec::with_capacity(chunk_samples * 2);
        let tx = tx;
        let running = running;

        move |mono_f32: Vec<f32>| {
            if !running.load(Ordering::SeqCst) {
                return;
            }
            let resampled = if need_resample {
                linear_resample(&mono_f32, resample_ratio)
            } else {
                mono_f32
            };
            buffer.extend_from_slice(&resampled);
            while buffer.len() >= chunk_samples {
                let chunk: Vec<f32> = buffer.drain(..chunk_samples).collect();
                let _ = tx.send(AudioChunk {
                    samples: chunk,
                    sample_rate: 16000,
                });
            }
        }
    };

    let stream = match sample_format {
        SampleFormat::F32 => {
            let mut process = make_processor();
            device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                            .collect();
                        process(mono);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| e.to_string())?
        }
        SampleFormat::I16 => {
            let mut process = make_processor();
            device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|frame| {
                                frame.iter().map(|&s| s as f32 / i16::MAX as f32).sum::<f32>()
                                    / channels as f32
                            })
                            .collect();
                        process(mono);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| e.to_string())?
        }
        _ => return Err(format!("Unsupported sample format: {:?}", sample_format)),
    };

    stream.play().map_err(|e| format!("Failed to start audio stream: {}", e))?;
    log::info!(
        "[Audio] Stream started successfully — device: {:?}, format: {:?}, rate: {}, channels: {}",
        device.name().unwrap_or_default(),
        sample_format,
        native_rate,
        channels
    );
    Ok(stream)
}

/// Simple linear interpolation resampler
fn linear_resample(input: &[f32], ratio: f64) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let output_len = (input.len() as f64 * ratio) as usize;
    let mut output = Vec::with_capacity(output_len);
    for i in 0..output_len {
        let src_pos = i as f64 / ratio;
        let src_idx = src_pos as usize;
        let frac = (src_pos - src_idx as f64) as f32;
        let sample = if src_idx + 1 < input.len() {
            input[src_idx] * (1.0 - frac) + input[src_idx + 1] * frac
        } else {
            input[src_idx.min(input.len() - 1)]
        };
        output.push(sample);
    }
    output
}

/// Encode f32 PCM samples as WAV bytes (16-bit mono)
/// This is a public utility for use by ASR and end-to-end engines.
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len();
    let byte_rate = sample_rate * 2; // 16-bit mono
    let data_size = (num_samples * 2) as u32;
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt subchunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // subchunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data subchunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    // Convert f32 to i16 PCM
    for &sample in samples {
        let clamped = sample.max(-1.0).min(1.0);
        let val = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&val.to_le_bytes());
    }

    buf
}

/// Encode WAV and convert to base64 string
pub fn encode_wav_base64(samples: &[f32], sample_rate: u32) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let wav_bytes = encode_wav(samples, sample_rate);
    STANDARD.encode(&wav_bytes)
}
