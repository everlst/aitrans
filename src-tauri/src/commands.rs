use crate::audio::AudioCapture;
use crate::config::{AppConfig, ConfigManager};
use crate::pipeline::Pipeline;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Mutex};

/// Shared application state
pub struct AppState {
    pub config: Arc<ConfigManager>,
    pub audio: Arc<AudioCapture>,
    pub pipeline_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Cancel flag shared with the pipeline — set to true to gracefully stop reconnection.
    pub pipeline_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

// ───────────── Config Commands ─────────────

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.get())
}

#[tauri::command]
pub fn save_config(app: AppHandle, config: AppConfig, state: State<'_, AppState>) -> Result<(), String> {
    state.config.update(config.clone())?;
    // Notify all windows (especially overlay) about config change
    let _ = app.emit("config-changed", &config);
    Ok(())
}

// ───────────── Audio Commands ─────────────

#[tauri::command]
pub fn list_audio_devices() -> Result<Vec<String>, String> {
    AudioCapture::list_devices()
}

// ───────────── Pipeline Commands ─────────────

#[tauri::command]
pub async fn start_pipeline(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Stop existing pipeline if any
    stop_pipeline(state.clone()).await?;

    let (audio_tx, audio_rx) = mpsc::unbounded_channel();

    // Start audio capture (dispatches based on audio source: microphone/system or app:<bundle_id>)
    let cfg = state.config.get();
    state.audio.start_with_source(&cfg.audio.source, &cfg.audio.device_name, cfg.audio.chunk_duration_ms, audio_tx)?;

    // Start processing pipeline
    let pipeline = Pipeline::new(state.config.clone());
    let cancel_flag = pipeline.cancel_flag();
    let handle = tokio::spawn(async move {
        pipeline.run(audio_rx, app).await;
    });

    let mut ph = state.pipeline_handle.lock().await;
    *ph = Some(handle);

    let mut pc = state.pipeline_cancel.lock().await;
    *pc = Some(cancel_flag);

    Ok(())
}

#[tauri::command]
pub async fn stop_pipeline(state: State<'_, AppState>) -> Result<(), String> {
    // Signal the pipeline to stop reconnecting
    {
        let pc = state.pipeline_cancel.lock().await;
        if let Some(cancel) = pc.as_ref() {
            cancel.store(true, Ordering::SeqCst);
        }
    }

    state.audio.stop();

    let mut ph = state.pipeline_handle.lock().await;
    if let Some(handle) = ph.take() {
        handle.abort();
    }

    // Clear the cancel flag
    let mut pc = state.pipeline_cancel.lock().await;
    *pc = None;

    Ok(())
}

#[tauri::command]
pub async fn is_pipeline_running(state: State<'_, AppState>) -> Result<bool, String> {
    if !state.audio.is_running() {
        return Ok(false);
    }
    let ph = state.pipeline_handle.lock().await;
    Ok(match ph.as_ref() {
        Some(handle) => !handle.is_finished(),
        None => false,
    })
}

// ───────────── Hotword Vocabulary Commands ─────────────

use crate::hotwords::{Hotword, VocabularyClient, VocabularyDetail, VocabularyInfo};

#[tauri::command]
pub async fn create_vocabulary(
    state: State<'_, AppState>,
    prefix: String,
    vocabulary: Vec<Hotword>,
) -> Result<String, String> {
    let cfg = state.config.get();
    let api_key = &cfg.gummy.api_key;
    if api_key.is_empty() {
        return Err("API Key 未配置".into());
    }
    let target_model = &cfg.gummy.model;
    VocabularyClient::create_vocabulary(api_key, target_model, &prefix, &vocabulary).await
}

#[tauri::command]
pub async fn list_vocabularies(
    state: State<'_, AppState>,
) -> Result<Vec<VocabularyInfo>, String> {
    let cfg = state.config.get();
    let api_key = &cfg.gummy.api_key;
    if api_key.is_empty() {
        return Err("API Key 未配置".into());
    }
    VocabularyClient::list_vocabularies(api_key, None, 0, 100).await
}

#[tauri::command]
pub async fn query_vocabulary(
    state: State<'_, AppState>,
    vocabulary_id: String,
) -> Result<VocabularyDetail, String> {
    let cfg = state.config.get();
    let api_key = &cfg.gummy.api_key;
    if api_key.is_empty() {
        return Err("API Key 未配置".into());
    }
    VocabularyClient::query_vocabulary(api_key, &vocabulary_id).await
}

#[tauri::command]
pub async fn update_vocabulary(
    state: State<'_, AppState>,
    vocabulary_id: String,
    vocabulary: Vec<Hotword>,
) -> Result<(), String> {
    let cfg = state.config.get();
    let api_key = &cfg.gummy.api_key;
    if api_key.is_empty() {
        return Err("API Key 未配置".into());
    }
    VocabularyClient::update_vocabulary(api_key, &vocabulary_id, &vocabulary).await
}

#[tauri::command]
pub async fn delete_vocabulary(
    state: State<'_, AppState>,
    vocabulary_id: String,
) -> Result<(), String> {
    let cfg = state.config.get();
    let api_key = &cfg.gummy.api_key;
    if api_key.is_empty() {
        return Err("API Key 未配置".into());
    }
    VocabularyClient::delete_vocabulary(api_key, &vocabulary_id).await
}
