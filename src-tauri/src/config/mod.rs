use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

/// Application configuration - persisted as JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Audio settings
    pub audio: AudioConfig,
    /// Gummy real-time speech translation settings (阿里云百炼)
    pub gummy: GummyConfig,
    /// Overlay display settings
    pub overlay: OverlayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Audio source: "system"
    pub source: String,
    /// Specific device name (empty = default)
    pub device_name: String,
    /// Sample rate for processing (default 16000)
    pub sample_rate: u32,
    /// Audio chunk duration in milliseconds (20–2000, default 160)
    /// Smaller values reduce latency; larger values reduce network overhead.
    pub chunk_duration_ms: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            source: "system".into(),
            device_name: String::new(),
            sample_rate: 16000,
            chunk_duration_ms: 160,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// Display mode: "bilingual" or "target_only"
    pub display_mode: String,
    /// Font family
    pub font_family: String,
    /// Font size in pixels
    pub font_size: u32,
    /// Overlay window opacity (0.0 - 1.0)
    pub opacity: f32,
    /// Background color (CSS format)
    pub background_color: String,
    /// Text color
    pub text_color: String,
    /// Source text color (for bilingual mode)
    pub source_text_color: String,
    /// Max lines to display
    pub max_lines: u32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            display_mode: "bilingual".into(),
            font_family: "Times New Roman".into(),
            font_size: 18,
            opacity: 0.85,
            background_color: "rgba(0, 0, 0, 0.80)".into(),
            text_color: "#FFFFFF".into(),
            source_text_color: "#AAAAAA".into(),
            max_lines: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GummyConfig {
    /// DashScope API key for Gummy service
    pub api_key: String,
    /// Model name (default: gummy-realtime-v1)
    pub model: String,
    /// Source language: "zh", "en", "ja", "ko", "auto", etc.
    pub source_language: String,
    /// Target translation languages, e.g. ["en"], ["zh", "ja"]
    pub target_languages: Vec<String>,
    /// Hotword vocabulary ID (created via DashScope API)
    pub vocabulary_id: Option<String>,
    /// VAD end-of-sentence silence threshold in milliseconds (200–6000, default 800).
    /// When silence after speech exceeds this duration, the system considers the sentence ended.
    /// Increase this value for speakers with slow pace or frequent pauses.
    pub max_end_silence: u32,
}

impl Default for GummyConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "gummy-realtime-v1".to_string(),
            source_language: "auto".to_string(),
            target_languages: vec!["zh".to_string()],
            vocabulary_id: None,
            max_end_silence: 800,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            audio: AudioConfig::default(),
            gummy: GummyConfig::default(),
            overlay: OverlayConfig::default(),
        }
    }
}

/// Thread-safe config manager
pub struct ConfigManager {
    config: RwLock<AppConfig>,
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("aitrans");
        fs::create_dir_all(&config_dir).ok();
        let config_path = config_dir.join("config.json");

        let config = if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
                Err(_) => AppConfig::default(),
            }
        } else {
            AppConfig::default()
        };

        let mgr = Self {
            config: RwLock::new(config),
            config_path,
        };
        mgr.save().ok();
        mgr
    }

    pub fn get(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    pub fn update(&self, new_config: AppConfig) -> Result<(), String> {
        {
            let mut cfg = self.config.write().map_err(|e| e.to_string())?;
            *cfg = new_config;
        }
        self.save()
    }

    pub fn save(&self) -> Result<(), String> {
        let cfg = self.config.read().map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&*cfg).map_err(|e| e.to_string())?;
        fs::write(&self.config_path, json).map_err(|e| e.to_string())?;
        Ok(())
    }
}
