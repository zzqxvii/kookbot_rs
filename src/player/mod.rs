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

#[derive(Debug, Clone)]
pub struct VoiceStreamingInfo {
    pub ip: String,
    pub port: u16,
    pub rtcp_port: u16,
    pub ssrc: u32,
    pub bit_rate: i32,
    pub sample_rate: u32,
    pub channels: usize,
}
