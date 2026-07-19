use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::core::error::{BotError, Result};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ConnectionMode {
    #[default]
    Websocket,
    Webhook,
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
    pub webhook: WebhookConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub music: MusicConfig,
    #[serde(default)]
    pub player: PlayerConfig,
    /// 配置文件路径（运行时注入，不从 TOML 反序列化）
    #[serde(skip, default)]
    pub config_path: Option<PathBuf>,
}

impl BotConfig {
    /// 检查用户是否是管理员
    pub fn is_admin(&self, user_id: &str) -> bool {
        self.admins.is_empty() || self.admins.iter().any(|a| a == user_id)
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
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            host: default_webhook_host(),
            port: default_webhook_port(),
            path: default_webhook_path(),
            verify_token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AudioConfig {
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_bitrate")]
    pub bit_rate: i32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            bit_rate: default_bitrate(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            timeout: default_timeout(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MusicConfig {
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    #[serde(default = "default_max_cache_size")]
    pub max_cache_size_mb: u64,
    #[serde(default = "default_netease_api_url")]
    pub netease_api_url: String,
    pub netease_cookie: Option<String>,
    #[serde(default = "default_qqmusic_api_url")]
    pub qqmusic_api_url: String,
    pub qqmusic_cookie: Option<String>,
    #[serde(default = "default_bilibili_api_url")]
    pub bilibili_api_url: String,
    pub bilibili_cookie: Option<String>,
}
impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir(),
            max_cache_size_mb: default_max_cache_size(),
            netease_api_url: default_netease_api_url(),
            netease_cookie: None,
            qqmusic_api_url: default_qqmusic_api_url(),
            qqmusic_cookie: None,
            bilibili_api_url: default_bilibili_api_url(),
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
    #[serde(default = "default_preload_count")]
    pub preload_count: usize,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            max_queue_size: default_max_queue_size(),
            allow_duplicates: false,
            preload_count: default_preload_count(),
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
    128000
}

fn default_timeout() -> u64 {
    30
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

fn default_cache_dir() -> String {
    "./cache".to_string()
}
fn default_max_cache_size() -> u64 {
    1024
}
fn default_netease_api_url() -> String {
    "http://localhost:3000".to_string()
}

fn default_qqmusic_api_url() -> String {
    "http://localhost:3300".to_string()
}

fn default_bilibili_api_url() -> String {
    "http://localhost:3400".to_string()
}

fn default_max_queue_size() -> usize {
    100
}

fn default_preload_count() -> usize {
    2
}

impl BotConfig {
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| BotError::ConfigError(format!("无法读取配置文件: {}", e)))?;

        let mut config: BotConfig = toml::from_str(&content)
            .map_err(|e| BotError::ConfigError(format!("解析配置文件失败: {}", e)))?;

        config.config_path = Some(path.as_ref().to_path_buf());
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
