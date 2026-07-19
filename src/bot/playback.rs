//! 共享播放逻辑 — 从 music.rs / qqmusic.rs / bilibili.rs 提取的公共代码
//!
//! 提供单曲播放（URL 模式）、歌单 stdin 管道播放、预下载槽位、
//! 非阻塞卡片更新等通用功能。

use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Notify};
use tracing::{debug, error, info, warn};

use crate::api::KookClient;
use crate::common::card::{
    build_play_card, PlayCardData, PlayMusic, QueueMusic, Sender as CardSender,
};
use crate::common::play_state::PlayState;
use crate::player::{Music, VoiceStreamingInfo};

// ── 预下载结果 ───────────────────────────────────────────────

/// 预下载完成的歌曲：文件路径 + 结构化信息（用于卡片展示）
#[derive(Debug, Clone)]
pub struct PreDownloadedSong {
    pub file_path: String,
    pub music: Music,
}

// ── 预下载槽位 ───────────────────────────────────────────────

/// 跨线程预下载槽位：async 下载线程写入，blocking 播放线程等待并取出。
///
/// 使用 `Notify` 在下载完成时通知等待方，避免轮询或阻塞式 API 调用。
pub struct PreDownloadSlot {
    result: Mutex<Option<Option<PreDownloadedSong>>>,
    ready: Notify,
}

impl Default for PreDownloadSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl PreDownloadSlot {
    pub fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Notify::new(),
        }
    }

    /// async 上下文：下载完成后存入结果并通知等待方。
    pub fn store(&self, song: Option<PreDownloadedSong>) {
        if let Ok(mut guard) = self.result.lock() {
            *guard = Some(song);
        }
        self.ready.notify_one();
    }

    /// blocking 上下文：等待预下载完成并取出结果。
    ///
    /// 如果结果已就绪则立即返回（常见情况：下载在上首歌播放期间已完成）；
    /// 否则通过 `rt.block_on(notified())` 高效等待。
    pub fn take_blocking(&self, rt: &tokio::runtime::Handle) -> Option<PreDownloadedSong> {
        let mut guard = self.result.lock().expect("PreDownloadSlot lock poisoned");
        if guard.is_none() {
            drop(guard);
            rt.block_on(self.ready.notified());
            guard = self.result.lock().expect("PreDownloadSlot lock poisoned");
        }
        guard.take().and_then(|o| o)
    }
}

// ── 非阻塞卡片更新 ───────────────────────────────────────────

/// 发送给卡片更新任务的请求（所有数据已就绪，无需额外 API 调用）
#[derive(Debug, Clone)]
pub struct CardUpdateRequest {
    pub current: PlayMusic,
    pub queue: Vec<QueueMusic>,
    pub queue_total: usize,
    pub channel_id: String,
    pub duration_secs: u64,
}

/// 启动后台卡片更新任务。
///
/// 接收 [`CardUpdateRequest`]，异步发送卡片消息并删除旧卡片。
/// 所有 I/O 在 async 上下文中完成，不阻塞音频推流线程。
pub fn spawn_card_updater(
    api_client: Arc<KookClient>,
    play_state: Arc<PlayState>,
) -> mpsc::UnboundedSender<CardUpdateRequest> {
    let (tx, mut rx) = mpsc::unbounded_channel::<CardUpdateRequest>();

    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            let mut data = PlayCardData::new(req.current);
            let total = if req.queue_total > 0 {
                req.queue_total + 1
            } else {
                1
            };
            data = data.with_queue(req.queue, total);

            let json = build_play_card(&data);

            if let Some(old) = play_state.take_play_msg_id() {
                let _ = api_client.delete_message(&old).await;
            }

            match api_client.send_card_message(&req.channel_id, &json).await {
                Ok(msg_id) => {
                    play_state.set_play_msg_id(msg_id);
                    play_state.set_current_song_duration(req.duration_secs);
                }
                Err(e) => {
                    warn!("发送播放卡片失败: {}", e);
                }
            }
        }
        debug!("卡片更新任务退出");
    });

    tx
}

/// 帮助函数：将 [`Music`] 转为卡片用的 [`PlayMusic`]
pub fn music_to_play_music(music: &Music, sender_nick: &str) -> PlayMusic {
    PlayMusic {
        title: music.title.clone(),
        author: music.author.clone(),
        platform: music.platform.clone(),
        pic_url: music.pic_url.clone(),
        sender: CardSender {
            nick_name: sender_nick.to_string(),
            avatar_url: None,
        },
    }
}

/// 帮助函数：将 [`Music`] 转为队列用的 [`QueueMusic`]
pub fn music_to_queue_music(music: &Music) -> QueueMusic {
    QueueMusic {
        title: music.title.clone(),
        author: music.author.clone(),
        platform: music.platform.clone(),
        pic_url: music.pic_url.clone(),
        sender: CardSender {
            nick_name: String::new(),
            avatar_url: None,
        },
    }
}

// ── 单曲播放（URL 模式）─────────────────────────────────────

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
