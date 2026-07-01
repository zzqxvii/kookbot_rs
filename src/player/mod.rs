mod manager;
mod playlist;
mod preloader;
mod queue;

pub use manager::VoiceManager;
pub use playlist::{Playlist, PlayMode};
pub use preloader::{PreloadManager, PreloadStatus, PreloadTask};
pub use queue::{QueueItem, QueueItemStatus, QueueManager, QueueConfig};
pub use crate::common::card::Sender;


use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Music {
    pub title: String,
    pub author: String,
    pub pic_url: String,
    pub platform: String,
    pub source_url: Option<String>,
    pub duration: Option<u64>,
    pub sender: Sender,
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
            sender: Sender::default(),
        }
    }
}

/// 语音推流信息
///
/// 用于 RTP 音频推流的连接参数
#[derive(Debug, Clone)]
pub struct VoiceStreamingInfo {
    /// 媒体服务器 IP
    pub ip: String,
    /// 媒体服务器端口
    pub port: u16,
    /// RTCP 端口
    pub rtcp_port: u16,
    /// 是否使用 RTCP MUX
    pub rtcp_mux: bool,
    /// SSRC
    pub ssrc: u32,
    /// Payload Type
    pub pt: u8,
    /// 比特率
    pub bit_rate: i32,
    /// 采样率
    pub sample_rate: u32,
    /// 声道数
    pub channels: usize,
}

impl VoiceStreamingInfo {
    pub fn from_conn(conn: &crate::common::models::VoiceConnectionInfo, fallback_bitrate: i32) -> Self {
        let ip = conn.ip.clone().unwrap_or_default();
        let port = conn.port.unwrap_or(0) as u16;
        Self {
            ip: ip.clone(),
            port,
            rtcp_port: conn.rtcp_port.map(|p| p as u16).unwrap_or(port + 1),
            rtcp_mux: conn.rtcp_mux.unwrap_or(true),
            ssrc: conn.audio_ssrc.map(|s| s as u32).unwrap_or(1111),
            pt: conn.audio_pt.map(|p| p as u8).unwrap_or(111),
            bit_rate: conn.bitrate.unwrap_or(fallback_bitrate),
            sample_rate: 48000,
            channels: 2,
        }
    }
}
