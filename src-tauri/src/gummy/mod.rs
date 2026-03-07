//! Gummy real-time speech translation via DashScope WebSocket API
//!
//! Uses Alibaba Cloud's `gummy-realtime-v1` model for simultaneous
//! speech recognition and translation over a persistent WebSocket connection.
//!
//! Protocol reference:
//!   https://help.aliyun.com/document_detail/2869339.html

use crate::audio::AudioChunk;
use crate::config::GummyConfig;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const DASHSCOPE_WS_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";

/// Maximum number of consecutive reconnection attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 5;

/// Base delay for exponential backoff between reconnection attempts (in seconds).
const RECONNECT_BASE_DELAY_SECS: u64 = 1;

/// Interval for WebSocket ping heartbeats (in seconds).
const HEARTBEAT_INTERVAL_SECS: u64 = 15;

/// Translation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResult {
    /// Original text (transcription)
    pub source: String,
    /// Translated text
    pub target: String,
    /// Whether this is a streaming partial result
    pub is_partial: bool,
}

// ─────────── Send Messages ───────────

/// Header for outgoing messages (run-task / finish-task)
#[derive(Serialize)]
struct SendHeader {
    action: String,
    task_id: String,
    streaming: String,
}

#[derive(Serialize)]
struct RunTaskMessage {
    header: SendHeader,
    payload: RunTaskPayload,
}

#[derive(Serialize)]
struct RunTaskPayload {
    task_group: String,
    task: String,
    function: String,
    model: String,
    parameters: GummyParameters,
    input: serde_json::Value,
}

#[derive(Serialize)]
struct GummyParameters {
    format: String,
    sample_rate: u32,
    source_language: String,
    transcription_enabled: bool,
    translation_enabled: bool,
    translation_target_languages: Vec<String>,
    /// Hotword vocabulary ID for custom hotwords
    #[serde(skip_serializing_if = "Option::is_none")]
    vocabulary_id: Option<String>,
    /// VAD end-of-sentence silence threshold in ms (200–6000)
    max_end_silence: u32,
}

#[derive(Serialize)]
struct FinishTaskMessage {
    header: SendHeader,
    payload: FinishTaskPayload,
}

#[derive(Serialize)]
struct FinishTaskPayload {
    input: serde_json::Value,
}

// ─────────── Receive Messages ───────────

#[derive(Deserialize, Debug)]
struct WsResponse {
    header: ResponseHeader,
    #[serde(default)]
    payload: Option<ResponsePayload>,
}

#[derive(Deserialize, Debug)]
struct ResponseHeader {
    #[serde(default)]
    event: String,
    #[serde(default)]
    task_id: String,
    /// Error code — API uses "error_code" in task-failed events
    #[serde(default, alias = "code")]
    error_code: Option<String>,
    /// Error message — API uses "error_message" in task-failed events
    #[serde(default, alias = "message")]
    error_message: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ResponsePayload {
    #[serde(default)]
    output: Option<OutputData>,
}

#[derive(Deserialize, Debug)]
struct OutputData {
    #[serde(default)]
    transcription: Option<TranscriptionData>,
    #[serde(default)]
    translations: Option<Vec<TranslationData>>,
}

#[derive(Deserialize, Debug)]
struct TranscriptionData {
    #[serde(default)]
    text: String,
    /// True when this is the final result for a sentence
    #[serde(default)]
    sentence_end: bool,
    #[serde(default)]
    sentence_id: Option<u32>,
}

#[derive(Deserialize, Debug)]
struct TranslationData {
    /// Language code of the translation (field name is "lang" in API)
    #[serde(default, alias = "language")]
    lang: String,
    #[serde(default)]
    text: String,
    /// True when this is the final result for a sentence
    #[serde(default)]
    sentence_end: bool,
    #[serde(default)]
    sentence_id: Option<u32>,
}

/// Log event types emitted during a Gummy session
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum GummyLogEvent {
    #[serde(rename = "info")]
    Info { message: String },
    #[serde(rename = "audio")]
    AudioSent { energy: f32, samples: usize },
    #[serde(rename = "result")]
    ResultReceived {
        source: String,
        target: String,
        is_partial: bool,
    },
}

/// Gummy real-time translation client
pub struct GummyClient;

/// Result of a single WebSocket session attempt.
enum SessionOutcome {
    /// The session ended normally (task-finished / connection closed) and can be retried.
    Disconnected(String),
    /// A fatal error occurred — do not retry.
    Fatal(String),
    /// The audio channel was closed (pipeline stopped) — do not retry.
    AudioClosed,
}

impl GummyClient {
    /// Run Gummy real-time translation session **with automatic reconnection**.
    ///
    /// When the WebSocket session ends (server timeout, task-finished, network
    /// error, etc.), this method will automatically reconnect and resume
    /// streaming audio — up to `MAX_RECONNECT_ATTEMPTS` consecutive failures.
    /// The counter resets whenever a session successfully starts (task-started).
    pub async fn run_session(
        config: &GummyConfig,
        audio_rx: mpsc::UnboundedReceiver<AudioChunk>,
        result_tx: mpsc::UnboundedSender<TranslationResult>,
        error_tx: mpsc::UnboundedSender<String>,
        log_tx: mpsc::UnboundedSender<GummyLogEvent>,
        cancel: Arc<AtomicBool>,
    ) {
        // We wrap audio_rx in an Arc<Mutex> so it can be reused across reconnections.
        let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
        let mut consecutive_failures: u32 = 0;

        loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = log_tx.send(GummyLogEvent::Info {
                    message: "会话已被取消".into(),
                });
                break;
            }

            let outcome = Self::run_single_session(
                config,
                audio_rx.clone(),
                &result_tx,
                &error_tx,
                &log_tx,
                &cancel,
            )
            .await;

            match outcome {
                SessionOutcome::AudioClosed => {
                    // Pipeline stopped normally — exit without error.
                    let _ = log_tx.send(GummyLogEvent::Info {
                        message: "音频流已关闭，会话结束".into(),
                    });
                    break;
                }
                SessionOutcome::Fatal(reason) => {
                    let _ = error_tx.send(format!("致命错误，停止重连: {}", reason));
                    break;
                }
                SessionOutcome::Disconnected(reason) => {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    consecutive_failures += 1;
                    if consecutive_failures > MAX_RECONNECT_ATTEMPTS {
                        let _ = error_tx.send(format!(
                            "已连续重连失败 {} 次，停止重连: {}",
                            MAX_RECONNECT_ATTEMPTS, reason
                        ));
                        break;
                    }

                    let delay = RECONNECT_BASE_DELAY_SECS * (1u64 << (consecutive_failures - 1).min(4));
                    let _ = log_tx.send(GummyLogEvent::Info {
                        message: format!(
                            "⚠️ 会话断开 ({}), {} 秒后第 {}/{} 次重连...",
                            reason, delay, consecutive_failures, MAX_RECONNECT_ATTEMPTS
                        ),
                    });
                    let _ = error_tx.send(format!(
                        "连接断开: {}，{} 秒后重连 ({}/{})",
                        reason, delay, consecutive_failures, MAX_RECONNECT_ATTEMPTS
                    ));

                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

                    // Drain stale audio data accumulated during the delay
                    {
                        let mut rx = audio_rx.lock().await;
                        let mut drained = 0u64;
                        while rx.try_recv().is_ok() {
                            drained += 1;
                        }
                        if drained > 0 {
                            log::info!("Gummy: drained {} stale audio chunks before reconnect", drained);
                        }
                    }
                }
            }
        }
    }

    /// Execute a single WebSocket session. Returns how it ended.
    async fn run_single_session(
        config: &GummyConfig,
        audio_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<AudioChunk>>>,
        result_tx: &mpsc::UnboundedSender<TranslationResult>,
        error_tx: &mpsc::UnboundedSender<String>,
        log_tx: &mpsc::UnboundedSender<GummyLogEvent>,
        cancel: &Arc<AtomicBool>,
    ) -> SessionOutcome {
        let task_id = uuid::Uuid::new_v4()
            .to_string()
            .replace('-', "");

        let _ = log_tx.send(GummyLogEvent::Info {
            message: format!(
                "正在连接 DashScope WebSocket... (task_id={})",
                &task_id[..8]
            ),
        });

        // Build WebSocket request with auth header
        let ws_url = url::Url::parse(DASHSCOPE_WS_URL).expect("invalid WS URL");
        let request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(DASHSCOPE_WS_URL)
            .header("Host", ws_url.host_str().unwrap_or("dashscope.aliyuncs.com"))
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .expect("failed to build WS request");

        // Connect
        let ws_stream = match tokio_tungstenite::connect_async(request).await {
            Ok((stream, _response)) => {
                let _ = log_tx.send(GummyLogEvent::Info {
                    message: "WebSocket 连接成功".into(),
                });
                stream
            }
            Err(e) => {
                return SessionOutcome::Disconnected(format!("WebSocket 连接失败: {}", e));
            }
        };

        let (mut ws_writer, mut ws_reader) = ws_stream.split();

        // Build and send run-task message
        let run_task = RunTaskMessage {
            header: SendHeader {
                action: "run-task".to_string(),
                task_id: task_id.clone(),
                streaming: "duplex".to_string(),
            },
            payload: RunTaskPayload {
                task_group: "audio".to_string(),
                task: "asr".to_string(),
                function: "recognition".to_string(),
                model: config.model.clone(),
                parameters: GummyParameters {
                    format: "pcm".to_string(),
                    sample_rate: 16000,
                    source_language: config.source_language.clone(),
                    transcription_enabled: true,
                    translation_enabled: true,
                    translation_target_languages: config.target_languages.clone(),
                    vocabulary_id: config.vocabulary_id.clone().filter(|s| !s.is_empty()),
                    max_end_silence: config.max_end_silence.clamp(200, 6000),
                },
                input: serde_json::json!({}),
            },
        };

        let run_task_json = serde_json::to_string(&run_task).unwrap();
        log::info!("Gummy: sending run-task: {}", run_task_json);
        let _ = log_tx.send(GummyLogEvent::Info {
            message: "发送 run-task 指令...".into(),
        });

        if let Err(e) = ws_writer.send(Message::Text(run_task_json.into())).await {
            return SessionOutcome::Disconnected(format!("发送 run-task 失败: {}", e));
        }

        // Wait for task-started event
        let task_started = loop {
            match ws_reader.next().await {
                Some(Ok(Message::Text(text))) => {
                    log::debug!("Gummy recv: {}", text);
                    match serde_json::from_str::<WsResponse>(&text) {
                        Ok(resp) => {
                            if resp.header.event == "task-started" {
                                let _ = log_tx.send(GummyLogEvent::Info {
                                    message: "✅ 任务已启动，开始发送音频".into(),
                                });
                                break true;
                            } else if resp.header.event == "task-failed" {
                                let code = resp.header.error_code.unwrap_or_default();
                                let msg = resp
                                    .header
                                    .error_message
                                    .unwrap_or_else(|| "unknown error".into());
                                // Auth errors or invalid params are fatal
                                if code.contains("InvalidApiKey")
                                    || code.contains("Unauthorized")
                                    || code.contains("InvalidParameter")
                                {
                                    return SessionOutcome::Fatal(format!(
                                        "任务启动失败: [{}] {}",
                                        code, msg
                                    ));
                                }
                                return SessionOutcome::Disconnected(format!(
                                    "任务启动失败: [{}] {}",
                                    code, msg
                                ));
                            }
                        }
                        Err(e) => {
                            log::warn!("Gummy: parse error: {} — raw: {}", e, text);
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    return SessionOutcome::Disconnected(
                        "连接在任务启动前关闭".into(),
                    );
                }
                Some(Err(e)) => {
                    return SessionOutcome::Disconnected(format!(
                        "WebSocket 错误: {}",
                        e
                    ));
                }
                _ => continue,
            }
        };

        if !task_started {
            return SessionOutcome::Disconnected("任务未能启动".into());
        }

        // --- Session successfully started: reset the reconnect counter via caller ---

        // Use a shared flag so audio_sender can signal "audio channel closed"
        let audio_closed = Arc::new(AtomicBool::new(false));
        let audio_closed_sender = audio_closed.clone();

        // Spawn a task to forward audio chunks as binary PCM frames with heartbeat
        let task_id_clone = task_id.clone();
        let log_tx_audio = log_tx.clone();
        let cancel_audio = cancel.clone();
        let audio_rx_clone = audio_rx.clone();
        let audio_sender = tokio::spawn(async move {
            let mut chunk_count: u64 = 0;
            let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
            heartbeat.tick().await; // skip the immediate first tick

            let mut rx = audio_rx_clone.lock().await;

            loop {
                if cancel_audio.load(Ordering::SeqCst) {
                    break;
                }

                tokio::select! {
                    chunk_opt = rx.recv() => {
                        match chunk_opt {
                            Some(chunk) => {
                                // Calculate energy for logging
                                let energy: f32 = if chunk.samples.is_empty() {
                                    0.0
                                } else {
                                    chunk.samples.iter().map(|s| s * s).sum::<f32>()
                                        / chunk.samples.len() as f32
                                };

                                // Convert f32 samples to i16 PCM bytes (little-endian)
                                let pcm_bytes: Vec<u8> = chunk
                                    .samples
                                    .iter()
                                    .flat_map(|&s| {
                                        let clamped = s.clamp(-1.0, 1.0);
                                        let i16_val = (clamped * 32767.0) as i16;
                                        i16_val.to_le_bytes()
                                    })
                                    .collect();

                                let sample_count = chunk.samples.len();

                                if let Err(e) = ws_writer.send(Message::Binary(pcm_bytes.into())).await {
                                    log::warn!("Gummy: failed to send audio frame: {}", e);
                                    break;
                                }

                                chunk_count += 1;
                                if chunk_count % 30 == 1 {
                                    let _ = log_tx_audio.send(GummyLogEvent::AudioSent {
                                        energy,
                                        samples: sample_count,
                                    });
                                }
                            }
                            None => {
                                // Audio channel closed — pipeline is stopping
                                audio_closed_sender.store(true, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                    _ = heartbeat.tick() => {
                        // Send WebSocket ping to keep the connection alive
                        if let Err(e) = ws_writer.send(Message::Ping(vec![].into())).await {
                            log::warn!("Gummy: failed to send ping: {}", e);
                            break;
                        }
                        log::debug!("Gummy: heartbeat ping sent");
                    }
                }
            }

            // Audio stream ended — send finish-task
            log::info!(
                "Gummy: audio sender ended after {} chunks, sending finish-task",
                chunk_count
            );
            let finish = FinishTaskMessage {
                header: SendHeader {
                    action: "finish-task".to_string(),
                    task_id: task_id_clone,
                    streaming: "duplex".to_string(),
                },
                payload: FinishTaskPayload {
                    input: serde_json::json!({}),
                },
            };
            let finish_json = serde_json::to_string(&finish).unwrap();
            let _ = ws_writer.send(Message::Text(finish_json.into())).await;
            let _ = ws_writer.close().await;
        });

        // Read results from WebSocket
        let mut disconnect_reason = String::from("unknown");
        while let Some(msg) = ws_reader.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    log::debug!("Gummy recv: {}", text);
                    match serde_json::from_str::<WsResponse>(&text) {
                        Ok(resp) => match resp.header.event.as_str() {
                            "result-generated" => {
                                // Reset reconnect counter on successful data
                                // (handled by caller via SessionOutcome)
                                if let Some(payload) = resp.payload {
                                    if let Some(output) = payload.output {
                                        let source_text = output
                                            .transcription
                                            .as_ref()
                                            .map(|t| t.text.clone())
                                            .unwrap_or_default();

                                        let target_text = output
                                            .translations
                                            .as_ref()
                                            .and_then(|ts| ts.first())
                                            .map(|t| t.text.clone())
                                            .unwrap_or_default();

                                        let is_partial = output
                                            .transcription
                                            .as_ref()
                                            .map(|t| !t.sentence_end)
                                            .unwrap_or(true);

                                        if !source_text.is_empty() || !target_text.is_empty() {
                                            let _ =
                                                log_tx.send(GummyLogEvent::ResultReceived {
                                                    source: source_text.clone(),
                                                    target: target_text.clone(),
                                                    is_partial,
                                                });
                                            let _ = result_tx.send(TranslationResult {
                                                source: source_text,
                                                target: target_text,
                                                is_partial,
                                            });
                                        }
                                    }
                                }
                            }
                            "task-finished" => {
                                let _ = log_tx.send(GummyLogEvent::Info {
                                    message: "✅ Gummy 任务完成".into(),
                                });
                                disconnect_reason = "task-finished".into();
                                break;
                            }
                            "task-failed" => {
                                let code = resp.header.error_code.unwrap_or_default();
                                let msg = resp
                                    .header
                                    .error_message
                                    .unwrap_or_else(|| "unknown error".into());
                                disconnect_reason =
                                    format!("task-failed: [{}] {}", code, msg);
                                let _ = error_tx.send(format!(
                                    "任务错误: [{}] {}",
                                    code, msg
                                ));
                                break;
                            }
                            other => {
                                log::debug!("Gummy: unhandled event: {}", other);
                            }
                        },
                        Err(e) => {
                            log::warn!("Gummy: parse error: {} — raw: {}", e, text);
                        }
                    }
                }
                Ok(Message::Pong(_)) => {
                    log::debug!("Gummy: pong received");
                }
                Ok(Message::Close(_)) => {
                    let _ = log_tx.send(GummyLogEvent::Info {
                        message: "WebSocket 连接已关闭".into(),
                    });
                    disconnect_reason = "server closed connection".into();
                    break;
                }
                Err(e) => {
                    disconnect_reason = format!("read error: {}", e);
                    let _ = error_tx.send(format!("WebSocket 读取错误: {}", e));
                    break;
                }
                _ => {}
            }
        }

        // Ensure audio sender is cleaned up
        audio_sender.abort();

        // If the audio channel was closed, the pipeline is stopping — don't reconnect.
        if audio_closed.load(Ordering::SeqCst) {
            return SessionOutcome::AudioClosed;
        }

        SessionOutcome::Disconnected(disconnect_reason)
    }
}
