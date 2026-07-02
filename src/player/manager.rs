use crate::api::client::KookClient;
use crate::audio::streamer::AudioStreamer;
use crate::common::play_state::PlayState;
use crate::core::config::BotConfig;
use crate::core::error::{BotError, Result};
use super::VoiceStreamingInfo;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// 语音频道管理器
///
/// 扁平化设计 — 每个可变字段有独立的 `tokio::sync::Mutex`，
/// 避免嵌套锁结构（旧版 `Mutex<VmInner>` → `Arc<Mutex<AudioStreamer>>`）。
/// 所有方法接受 `&self`，调用者持有 `Arc<VoiceManager>` 即可并发调用。
pub struct VoiceManager {
    kook_client: KookClient,
    config: BotConfig,
    /// 当前语音频道 ID
    current_channel: Mutex<Option<String>>,
    /// 音频流处理器（内部 `Arc<Mutex<>>` 封装 `AudioStreamer` 的 `&mut self` 方法）
    audio_streamer: Mutex<Option<Arc<Mutex<AudioStreamer>>>>,
    play_state: Arc<PlayState>,
}

impl VoiceManager {
    pub async fn new(config: &BotConfig, play_state: Arc<PlayState>) -> Result<Self> {
        let kook_client = KookClient::new(config)?;
        info!("语音管理器创建成功");
        Ok(Self {
            kook_client,
            config: config.clone(),
            current_channel: Mutex::new(None),
            audio_streamer: Mutex::new(None),
            play_state,
        })
    }

    /// 加入语音频道
    pub async fn join_channel(&self, channel_id: &str) -> Result<()> {
        {
            let channel = self.current_channel.lock().await;
            if channel.is_some() {
                warn!("已经在语音频道中，先离开当前频道");
                drop(channel);
                self.leave_channel().await?;
            }
        }

        info!("正在加入语音频道: {}", channel_id);

        let connection_info = self.kook_client.join_voice_channel(channel_id).await?;

        let port: u16 = connection_info.port.unwrap_or(0) as u16;
        let rtcp_port: u16 = connection_info
            .rtcp_port
            .map(|p| p as u16)
            .unwrap_or(port + 1);
        let rtcp_mux = connection_info.rtcp_mux.unwrap_or(true);
        let ssrc: u32 = connection_info.audio_ssrc.map(|s| s as u32).unwrap_or(1111);
        let pt: u8 = connection_info.audio_pt.map(|p| p as u8).unwrap_or(111);
        let bit_rate = connection_info.bitrate.unwrap_or(self.config.audio.bit_rate);
        let ip = connection_info.ip.clone().unwrap_or_default();

        info!(
            "成功加入频道，RTP 服务器: {}:{}, SSRC: {}, PT: {}, 比特率: {}kbps",
            ip, port, ssrc, pt, bit_rate / 1000
        );

        let streaming_info = VoiceStreamingInfo {
            ip,
            port,
            rtcp_port,
            rtcp_mux,
            ssrc,
            pt,
            bit_rate,
            sample_rate: 48000,
            channels: 2,
        };

        let audio_streamer = AudioStreamer::new(
            &streaming_info,
            self.config.audio.clone(),
            self.config.network.clone(),
            self.play_state.clone(),
        )?;

        *self.audio_streamer.lock().await = Some(Arc::new(Mutex::new(audio_streamer)));
        *self.current_channel.lock().await = Some(channel_id.to_string());

        info!("语音频道准备就绪");
        Ok(())
    }

    /// 离开语音频道
    pub async fn leave_channel(&self) -> Result<()> {
        // 先停止音频流
        if let Some(streamer) = self.audio_streamer.lock().await.as_ref() {
            streamer.lock().await.stop();
        }
        *self.audio_streamer.lock().await = None;

        if let Some(channel_id) = self.current_channel.lock().await.take() {
            info!("正在离开语音频道: {}", channel_id);
            if let Err(e) = self.kook_client.leave_voice_channel(&channel_id).await {
                warn!("离开频道 API 调用失败: {}", e);
            }
            info!("已离开语音频道");
        }

        Ok(())
    }

    /// 播放音频文件
    pub async fn play_file(&self, file_path: impl AsRef<Path>) -> Result<()> {
        let file_path = file_path.as_ref();

        {
            let channel = self.current_channel.lock().await;
            if channel.is_none() {
                return Err(BotError::NotInVoiceChannel);
            }
        }

        let streamer = {
            let guard = self.audio_streamer.lock().await;
            guard.as_ref().ok_or(BotError::StreamNotStarted)?.clone()
        };

        info!("开始播放文件: {:?}", file_path);

        let file_path = file_path.to_path_buf();
        tokio::spawn(async move {
            let mut streamer = streamer.lock().await;
            if let Err(e) = streamer.stream_file(&file_path).await {
                error!("播放文件失败: {}", e);
            }
        });

        Ok(())
    }

    /// 停止播放
    pub async fn stop(&self) {
        if let Some(streamer) = self.audio_streamer.lock().await.as_ref() {
            streamer.lock().await.stop();
        }
    }

    /// 获取当前频道 ID（try_lock 快照，非阻塞）
    pub fn current_channel(&self) -> Option<String> {
        self.current_channel.try_lock().ok().and_then(|g| g.clone())
    }

    /// 检查是否正在播放
    pub async fn is_playing(&self) -> bool {
        if let Some(streamer) = self.audio_streamer.lock().await.as_ref() {
            streamer.lock().await.is_running()
        } else {
            false
        }
    }
}
