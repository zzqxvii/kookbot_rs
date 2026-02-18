use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{BotError, Result};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionMode {
    Websocket,
    Webhook,
}

impl Default for ConnectionMode {
    fn default() -> Self {
        ConnectionMode::Websocket
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BotConfig {
    pub token: String,
    #[serde(default)]
    pub mode: ConnectionMode,
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default)]
    pub admins: Vec<String>,
    #[serde(default)]
    pub websocket: WebsocketConfig,
    #[serde(default)]
    pub webhook: WebhookConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub music: MusicConfig,
    #[serde(default)]
    pub player: PlayerConfig,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebsocketConfig {
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
    #[serde(default = "default_compress")]
    pub compress: bool,
    #[serde(default = "default_reconnect_attempts")]
    pub reconnect_attempts: u32,
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay: u64,
}

impl Default for WebsocketConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: default_heartbeat_interval(),
            compress: default_compress(),
            reconnect_attempts: default_reconnect_attempts(),
            reconnect_delay: default_reconnect_delay(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookConfig {
    #[serde(default = "default_webhook_host")]
    pub host: String,
    #[serde(default = "default_webhook_port")]
    pub port: u16,
    #[serde(default = "default_webhook_path")]
    pub path: String,
    pub verify_token: String,
    #[serde(default)]
    pub encrypt_key: Option<String>,
    #[serde(default)]
    pub use_ssl: bool,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            host: default_webhook_host(),
            port: default_webhook_port(),
            path: default_webhook_path(),
            verify_token: String::new(),
            encrypt_key: None,
            use_ssl: false,
            cert_path: None,
            key_path: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AudioConfig {
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_bitrate")]
    pub bit_rate: i32,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_channels")]
    pub channels: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            bit_rate: default_bitrate(),
            sample_rate: default_sample_rate(),
            channels: default_channels(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default = "default_retries")]
    pub retries: u32,
    #[serde(default = "default_packet_size")]
    pub packet_size: usize,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            timeout: default_timeout(),
            retries: default_retries(),
            packet_size: default_packet_size(),
            http_proxy: None,
            https_proxy: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MusicConfig {
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    #[serde(default = "default_max_cache_size")]
    pub max_cache_size: u64,
    pub netease_cookie: Option<String>,
    pub qqmusic_cookie: Option<String>,
    pub bilibili_cookie: Option<String>,
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir(),
            max_cache_size: default_max_cache_size(),
            netease_cookie: None,
            qqmusic_cookie: None,
            bilibili_cookie: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlayerConfig {
    #[serde(default = "default_max_queue_size")]
    pub max_queue_size: usize,
    #[serde(default)]
    pub allow_duplicates: bool,
    #[serde(default = "default_autoplay")]
    pub autoplay: bool,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default = "default_preload_count")]
    pub preload_count: usize,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            max_queue_size: default_max_queue_size(),
            allow_duplicates: false,
            autoplay: default_autoplay(),
            shuffle: false,
            preload_count: default_preload_count(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    pub file: Option<String>,
    #[serde(default = "default_log_console")]
    pub console: bool,
    #[serde(default = "default_log_format")]
    pub format: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file: None,
            console: default_log_console(),
            format: default_log_format(),
        }
    }
}

fn default_prefix() -> String {
    "/".to_string()
}
fn default_volume() -> f32 {
    0.5
}
fn default_bitrate() -> i32 {
    64000
}
fn default_sample_rate() -> u32 {
    48000
}
fn default_channels() -> usize {
    2
}
fn default_timeout() -> u64 {
    30
}
fn default_retries() -> u32 {
    3
}
fn default_packet_size() -> usize {
    1200
}
fn default_webhook_host() -> String {
    "0.0.0.0".to_string()
}
fn default_webhook_port() -> u16 {
    8080
}
fn default_webhook_path() -> String {
    "/webhook".to_string()
}
fn default_heartbeat_interval() -> u64 {
    30
}
fn default_compress() -> bool {
    true
}
fn default_reconnect_attempts() -> u32 {
    5
}
fn default_reconnect_delay() -> u64 {
    5
}
fn default_cache_dir() -> String {
    "./cache".to_string()
}
fn default_max_cache_size() -> u64 {
    1024
}
fn default_max_queue_size() -> usize {
    100
}
fn default_autoplay() -> bool {
    true
}
fn default_preload_count() -> usize {
    2
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_log_console() -> bool {
    true
}
fn default_log_format() -> String {
    "compact".to_string()
}

impl BotConfig {
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let content = fs::read_to_string(path)
            .map_err(|e| BotError::ConfigError(format!("无法读取配置文件: {}", e)))?;

        let config: BotConfig = toml::from_str(&content)
            .map_err(|e| BotError::ConfigError(format!("解析配置文件失败: {}", e)))?;

        config.validate()?;
        Ok(config)
    }

    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| BotError::ConfigError(format!("序列化配置失败: {}", e)))?;

        fs::write(path, content)
            .map_err(|e| BotError::ConfigError(format!("写入配置文件失败: {}", e)))?;

        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.token.is_empty() {
            return Err(BotError::ConfigError("Token 不能为空".to_string()));
        }

        if self.audio.volume < 0.0 || self.audio.volume > 1.0 {
            return Err(BotError::ConfigError(
                "音量必须在 0.0 到 1.0 之间".to_string(),
            ));
        }

        if self.mode == ConnectionMode::Webhook && self.webhook.verify_token.is_empty() {
            return Err(BotError::ConfigError(
                "Webhook 模式需要配置 verify_token".to_string(),
            ));
        }

        Ok(())
    }

    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut p| {
            p.push("kook-music-bot");
            p.push("config.toml");
            p
        })
    }
}
