use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{BotError, Result};

/// Bot 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BotConfig {
    /// Kook Bot Token
    pub token: String,
    /// 命令前缀
    #[serde(default = "default_prefix")]
    pub prefix: String,
    /// 管理员 ID 列表
    #[serde(default)]
    pub admins: Vec<String>,
    /// 音频配置
    #[serde(default)]
    pub audio: AudioConfig,
    /// 网络配置
    #[serde(default)]
    pub network: NetworkConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AudioConfig {
    /// 默认音量 (0.0 - 1.0)
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// 默认比特率
    #[serde(default = "default_bitrate")]
    pub bit_rate: i32,
    /// 采样率
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// 声道数
    #[serde(default = "default_channels")]
    pub channels: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    /// 连接超时(秒)
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// 重试次数
    #[serde(default = "default_retries")]
    pub retries: u32,
    /// RTP 包大小
    #[serde(default = "default_packet_size")]
    pub packet_size: usize,
}

// 默认值函数
fn default_prefix() -> String { "!".to_string() }
fn default_volume() -> f32 { 0.5 }
fn default_bitrate() -> i32 { 64000 }
fn default_sample_rate() -> u32 { 48000 }
fn default_channels() -> usize { 2 }
fn default_timeout() -> u64 { 30 }
fn default_retries() -> u32 { 3 }
fn default_packet_size() -> usize { 1200 }

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

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            timeout: default_timeout(),
            retries: default_retries(),
            packet_size: default_packet_size(),
        }
    }
}

impl BotConfig {
    /// 从文件加载配置
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let content = fs::read_to_string(path)
            .map_err(|e| BotError::ConfigError(format!("无法读取配置文件: {}", e)))?;

        let config: BotConfig = toml::from_str(&content)
            .map_err(|e| BotError::ConfigError(format!("解析配置文件失败: {}", e)))?;

        config.validate()?;
        Ok(config)
    }

    /// 保存配置到文件
    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| BotError::ConfigError(format!("序列化配置失败: {}", e)))?;

        fs::write(path, content)
            .map_err(|e| BotError::ConfigError(format!("写入配置文件失败: {}", e)))?;

        Ok(())
    }

    /// 验证配置
    fn validate(&self) -> Result<()> {
        if self.token.is_empty() {
            return Err(BotError::ConfigError("Token 不能为空".to_string()));
        }

        if self.audio.volume < 0.0 || self.audio.volume > 1.0 {
            return Err(BotError::ConfigError("音量必须在 0.0 到 1.0 之间".to_string()));
        }

        Ok(())
    }

    /// 获取默认配置文件路径
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut p| {
            p.push("kook-music-bot");
            p.push("config.toml");
            p
        })
    }

    /// 创建默认配置示例
    pub fn create_example() -> Self {
        Self {
            token: "你的 Kook Bot Token".to_string(),
            prefix: "!".to_string(),
            admins: vec!["你的用户ID".to_string()],
            audio: AudioConfig::default(),
            network: NetworkConfig::default(),
        }
    }
}