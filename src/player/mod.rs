mod manager;
mod playlist;
mod preloader;
mod queue;

pub use manager::VoiceManager;
pub use playlist::{Playlist, PlayMode};
pub use preloader::{PreloadManager, PreloadStatus, PreloadTask};
pub use queue::{QueueItem, QueueItemStatus, QueueManager, QueueConfig};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sender {
    pub nick_name: String,
    pub avatar_url: Option<String>,
}

impl Default for Sender {
    fn default() -> Self {
        Self {
            nick_name: "未知用户".to_string(),
            avatar_url: None,
        }
    }
}

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
