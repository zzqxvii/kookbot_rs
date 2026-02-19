use crate::api::client::KookClient;
use crate::audio::streamer::AudioStreamer;
use crate::core::config::BotConfig;
use crate::core::error::{BotError, Result};
use super::VoiceStreamingInfo;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// 语音频道管理器
pub struct VoiceManager {
    /// Kook API 客户端
    kook_client: KookClient,
    /// 配置
    config: BotConfig,
    /// 当前连接的频道 ID
    current_channel: Option<String>,
    /// 音频流处理器
    audio_streamer: Option<Arc<Mutex<AudioStreamer>>>,
}

impl VoiceManager {
    /// 创建新的语音管理器
    pub async fn new(config: &BotConfig) -> Result<Self> {
        let kook_client = KookClient::new(config)?;

        info!("语音管理器创建成功");

        Ok(Self {
            kook_client,
            config: config.clone(),
            current_channel: None,
            audio_streamer: None,
        })
    }

    /// 加入语音频道
    pub async fn join_channel(&mut self, channel_id: &str) -> Result<()> {
        if self.current_channel.is_some() {
            warn!("已经在语音频道中，先离开当前频道");
            self.leave_channel().await?;
        }

        info!("正在加入语音频道: {}", channel_id);

        // 调用 Kook API 加入频道
        let connection_info = self.kook_client.join_voice_channel(channel_id).await?;

        // 解析端口
        let port: u16 = connection_info.port.unwrap_or(0) as u16;
        
        let rtcp_port: u16 = connection_info.rtcp_port
            .map(|p| p as u16)
            .unwrap_or(port + 1);
        
        let rtcp_mux = connection_info.rtcp_mux.unwrap_or(true);
        
        let ssrc: u32 = connection_info.audio_ssrc
            .map(|s| s as u32)
            .unwrap_or(1111);
        
        let pt: u8 = connection_info.audio_pt
            .map(|p| p as u8)
            .unwrap_or(111);
        
        let bit_rate = connection_info.bitrate.unwrap_or(self.config.audio.bit_rate);
        
        let ip = connection_info.ip.clone().unwrap_or_default();

        info!(
            "成功加入频道，RTP 服务器: {}:{}, SSRC: {}, PT: {}, 比特率: {}kbps",
            ip, port, ssrc, pt, bit_rate / 1000
        );

        // 创建音频流处理器
        let streaming_info = VoiceStreamingInfo {
            ip,
            port,
            rtcp_port,
            rtcp_mux,
            ssrc,
            pt,
            bit_rate,
            sample_rate: 48000, // Kook 使用 48kHz
            channels: 2,        // 立体声
        };

        let audio_streamer = AudioStreamer::new(
            &streaming_info,
            self.config.audio.clone(),
            self.config.network.clone(),
        )?;

        self.audio_streamer = Some(Arc::new(Mutex::new(audio_streamer)));
        self.current_channel = Some(channel_id.to_string());

        info!("语音频道准备就绪");
        Ok(())
    }

    /// 离开语音频道
    pub async fn leave_channel(&mut self) -> Result<()> {
        // 停止音频流
        if let Some(ref streamer) = self.audio_streamer {
            let mut streamer = streamer.lock().await;
            streamer.stop();
        }
        self.audio_streamer = None;

        // 如果当前在频道中，调用 API 离开
        if let Some(ref channel_id) = self.current_channel {
            info!("正在离开语音频道: {}", channel_id);
            if let Err(e) = self.kook_client.leave_voice_channel(channel_id).await {
                warn!("离开频道 API 调用失败: {}", e);
            }
            info!("已离开语音频道");
        }

        self.current_channel = None;
        Ok(())
    }

    /// 播放音频文件
    pub async fn play_file(&mut self, file_path: impl AsRef<Path>) -> Result<()> {
        let file_path = file_path.as_ref();

        // 确保已经在语音频道
        if self.current_channel.is_none() {
            return Err(BotError::NotInVoiceChannel);
        }

        // 确保有音频流处理器
        let streamer = self
            .audio_streamer
            .as_ref()
            .ok_or(BotError::StreamNotStarted)?;

        info!("开始播放文件: {:?}", file_path);

        // 在后台任务中播放音频
        let streamer_clone = Arc::clone(streamer);
        let file_path = file_path.to_path_buf();

        tokio::spawn(async move {
            let mut streamer = streamer_clone.lock().await;
            if let Err(e) = streamer.stream_file(&file_path).await {
                error!("播放文件失败: {}", e);
            }
        });

        Ok(())
    }

    /// 停止播放
    pub async fn stop(&mut self) {
        if let Some(ref streamer) = self.audio_streamer {
            let mut streamer = streamer.lock().await;
            streamer.stop();
        }
    }

    /// 获取当前频道 ID
    pub fn current_channel(&self) -> Option<&str> {
        self.current_channel.as_deref()
    }

    /// 检查是否正在播放
    pub async fn is_playing(&self) -> bool {
        if let Some(ref streamer) = self.audio_streamer {
            let streamer = streamer.lock().await;
            streamer.is_running()
        } else {
            false
        }
    }
}
