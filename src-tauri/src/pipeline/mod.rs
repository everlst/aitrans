use crate::audio::AudioChunk;
use crate::config::ConfigManager;
use crate::gummy::{GummyClient, GummyLogEvent, TranslationResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

/// Events emitted to the frontend
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineEvent {
    pub event_type: String, // "vad", "asr", "translation", "error", "status", "log"
    pub data: serde_json::Value,
}

/// Main processing pipeline: Audio → Gummy (语音识别+翻译) → UI
pub struct Pipeline {
    config: Arc<ConfigManager>,
    cancel: Arc<AtomicBool>,
}

impl Pipeline {
    pub fn new(config: Arc<ConfigManager>) -> Self {
        Self {
            config,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a clone of the cancel flag so callers can signal cancellation.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Process audio chunks from the capture stream via Gummy
    pub async fn run(
        &self,
        audio_rx: mpsc::UnboundedReceiver<AudioChunk>,
        app_handle: AppHandle,
    ) {
        emit_event(
            &app_handle,
            "status",
            serde_json::json!({
                "message": "Pipeline started (Gummy)",
            }),
        );

        let cfg = self.config.get();

        if cfg.gummy.api_key.is_empty() {
            emit_event(
                &app_handle,
                "error",
                serde_json::json!({"message": "Gummy API Key 未配置，请在设置中填写 DashScope API Key"}),
            );
            return;
        }

        let (result_tx, mut result_rx) = mpsc::unbounded_channel::<TranslationResult>();
        let (error_tx, mut error_rx) = mpsc::unbounded_channel::<String>();
        let (log_tx, mut log_rx) = mpsc::unbounded_channel::<GummyLogEvent>();

        // Spawn Gummy WebSocket session
        let gummy_config = cfg.gummy.clone();
        let cancel = self.cancel.clone();
        let gummy_task = tokio::spawn(async move {
            GummyClient::run_session(&gummy_config, audio_rx, result_tx, error_tx, log_tx, cancel).await;
        });

        let app_err = app_handle.clone();
        let app_log = app_handle.clone();

        // Spawn error receiver
        let error_task = tokio::spawn(async move {
            while let Some(err_msg) = error_rx.recv().await {
                emit_event(
                    &app_err,
                    "error",
                    serde_json::json!({"message": format!("Gummy: {}", err_msg)}),
                );
            }
        });

        // Spawn log receiver — forward GummyLogEvents to frontend
        let log_task = tokio::spawn(async move {
            while let Some(log_event) = log_rx.recv().await {
                emit_event(
                    &app_log,
                    "log",
                    serde_json::to_value(&log_event).unwrap_or_default(),
                );
            }
        });

        // Process translation results
        while let Some(result) = result_rx.recv().await {
            // Emit ASR event with source (transcription) text
            if !result.source.is_empty() {
                emit_event(
                    &app_handle,
                    "asr",
                    serde_json::json!({
                        "text": result.source,
                        "language": null,
                    }),
                );
            }

            if !result.target.is_empty() || !result.source.is_empty() {
                emit_event(
                    &app_handle,
                    "translation",
                    serde_json::json!({
                        "source": result.source,
                        "target": result.target,
                        "is_partial": result.is_partial,
                    }),
                );
            }
        }

        gummy_task.await.ok();
        error_task.abort();
        log_task.abort();

        // Signal cancellation so any pending reconnect loop exits
        self.cancel.store(true, Ordering::SeqCst);

        emit_event(
            &app_handle,
            "status",
            serde_json::json!({"message": "Pipeline stopped"}),
        );

        // Emit a dedicated event so the frontend can reset the running state
        emit_event(
            &app_handle,
            "pipeline-stopped",
            serde_json::json!({}),
        );
    }
}

fn emit_event(app: &AppHandle, event_type: &str, data: serde_json::Value) {
    let event = PipelineEvent {
        event_type: event_type.to_string(),
        data,
    };
    let _ = app.emit("pipeline-event", &event);
}
