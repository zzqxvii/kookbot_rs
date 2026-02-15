use serde::{Deserialize, Serialize};

/// Kook API 通用响应结构
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KookResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

/// 语音频道连接信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceConnectionInfo {
    /// RTP 服务器 IP
    pub ip: String,
    /// RTP 服务器端口
    pub port: u16,
    /// RTCP 端口
    #[serde(rename = "rtcp_port")]
    pub rtcp_port: u16,
    /// 比特率
    #[serde(rename = "bitrate")]
    pub bit_rate: i32,
    /// SSRC
    pub ssrc: u32,
    /// 加密密钥
    pub key: Option<String>,
}

/// 语音频道状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum VoiceChannelStatus {
    /// 发送者不在频道
    SenderNotInChannel,
    /// Bot 不在频道
    BotNotInChannel,
    /// 在同一频道
    SameChannel,
    /// 在不同频道
    DifferentChannel,
}

/// 音乐信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Music {
    pub title: String,
    pub author: String,
    pub pic_url: String,
    pub platform: String,
    pub source_url: Option<String>,
    pub duration: Option<u64>,
}

impl Default for Music {
    fn default() -> Self {
        Self {
            title: "无结果".to_string(),
            author: "o.o".to_string(),
            pic_url: "https://img.kookapp.cn/assets/2023-07/bek0jyKtlt02i02s.gif".to_string(),
            platform: "虚空".to_string(),
            source_url: None,
            duration: None,
        }
    }
}

/// 用户加入的频道信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JoinedChannel {
    pub id: String,
    pub name: String,
    #[serde(rename = "user_id")]
    pub user_id: String,
    #[serde(rename = "guild_id")]
    pub guild_id: String,
    #[serde(rename = "channel_type")]
    pub channel_type: i32,
}

/// Kook API 用户对象
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(rename = "avatar")]
    pub avatar: Option<String>,
}

/// 播放队列项
#[derive(Debug, Clone)]
pub struct QueueItem {
    pub music: Music,
    pub requested_by: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// 语音流信息（用于创建音频流处理器）
#[derive(Debug, Clone)]
pub struct VoiceStreamingInfo {
    /// RTP 服务器 IP
    pub ip: String,
    /// RTP 服务器端口
    pub port: u16,
    /// RTCP 端口
    pub rtcp_port: u16,
    /// SSRC
    pub ssrc: u32,
    /// 比特率 (bps)
    pub bit_rate: i32,
    /// 采样率 (Hz)
    pub sample_rate: u32,
    /// 声道数
    pub channels: usize,
}