//! 共享播放逻辑 — 从 music.rs / qqmusic.rs / bilibili.rs 提取的公共代码
//!
//! 提供单曲播放（URL 模式）、歌单 stdin 管道播放等通用函数。

use std::sync::Arc;
use tracing::{error, info};

use crate::common::play_state::PlayState;
use crate::player::VoiceStreamingInfo;

/// 在后台线程中播放歌曲文件（URL 模式，用于单曲）。
///
/// 包含静音握手 → 创建 FFmpeg 流处理器 → 推流 → 等待完成的完整流程。
pub async fn play_song_file(
    file_path: String,
    ip: String,
    port: u16,
    streaming_info: VoiceStreamingInfo,
    play_state: Arc<PlayState>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        use crate::audio::{send_silence_handshake, FFmpegDirectStreamer, StreamerConfig};

        // 先发静音包握手，建立 UDP 连接
        send_silence_handshake(&ip, port);

        let mut streamer = match FFmpegDirectStreamer::new(
            StreamerConfig::from(&streaming_info),
            play_state.clone(),
        ) {
            Ok(s) => s,
            Err(e) => {
                error!("创建流处理器失败: {}", e);
                return;
            }
        };

        match streamer.start_stream_url(&file_path, &ip, port, streaming_info.rtcp_port) {
            Ok(_) => {
                let _ = streamer.wait();
                play_state.set_stopped();
                info!("🎵 歌曲播放完成");
            }
            Err(e) => {
                error!("推流失败: {}", e);
                play_state.set_stopped();
            }
        }
    })
}
