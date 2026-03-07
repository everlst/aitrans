mod audio;
mod commands;
mod config;
mod gummy;
mod hotwords;
mod pipeline;

use commands::AppState;
use config::ConfigManager;
use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let config = Arc::new(ConfigManager::new());
    let audio = Arc::new(audio::AudioCapture::new());

    let state = AppState {
        config,
        audio,
        pipeline_handle: tokio::sync::Mutex::new(None),
        pipeline_cancel: tokio::sync::Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::list_audio_devices,
            commands::start_pipeline,
            commands::stop_pipeline,
            commands::is_pipeline_running,
            commands::create_vocabulary,
            commands::list_vocabularies,
            commands::query_vocabulary,
            commands::update_vocabulary,
            commands::delete_vocabulary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
