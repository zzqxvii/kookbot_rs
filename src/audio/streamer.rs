use crate::audio::decoder::AudioDecoder;
use crate::audio::ffmpeg_encoder::{FFmpegOpusConfig, FFmpegOpusEncoder};
use crate::audio::ffmpeg_streamer::{FFmpegDirectStreamer, StreamerConfig};
use crate::audio::rtp::{RtpSender, RtpStats};
use crate::core::config::{AudioConfig, NetworkConfig};
use crate::core::error::Result;
use crate::player::VoiceStreamingInfo;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, trace, warn};

/// 音频流处理器
/// 负责将音频文件解码、编码并通过 RTP 发送
pub struct AudioStreamer {
    /// RTP 发送器
    rtp_sender: RtpSender,
    /// FFmpeg Opus 编码器
    opus_encoder: FFmpegOpusEncoder,
    /// FFmpeg 直接推流器 (用于 URL 流)
    direct_streamer: Option<FFmpegDirectStreamer>,
    /// 是否正在运行
    running: Arc<AtomicBool>,
    /// 音频配置
    audio_config: AudioConfig,
    /// 目标地址
    dest_addr: String,
    /// 流信息
    streaming_info: VoiceStreamingInfo,
}

impl AudioStreamer {
    /// 创建新的音频流处理器
    pub fn new(
        streaming_info: &VoiceStreamingInfo,
        audio_config: AudioConfig,
        _network_config: NetworkConfig,
    ) -> Result<Self> {
        // 构建目标地址
        let dest_addr = format!("{}:{}", streaming_info.ip, streaming_info.port);

        // 创建 RTP 发送器
        let rtp_sender = RtpSender::new(
            dest_addr.clone(),
            streaming_info.ssrc,
            streaming_info.pt,
            streaming_info.sample_rate,
        )?;

        // 创建 FFmpeg Opus 编码器配置
        let opus_config = crate::audio::ffmpeg_encoder::FFmpegOpusConfig {
            sample_rate: streaming_info.sample_rate,
            channels: streaming_info.channels,
            bit_rate: streaming_info.bit_rate,
            frame_duration_ms: 20,
            ffmpeg_path: None, // 使用系统 PATH 中的 FFmpeg
        };

        let opus_encoder = crate::audio::ffmpeg_encoder::FFmpegOpusEncoder::new(opus_config)?;

        info!(
            "音频流处理器创建成功: 目标={}, SSRC={}, PT={}, {}Hz, {} 声道, {}bps",
            dest_addr,
            streaming_info.ssrc,
            streaming_info.pt,
            streaming_info.sample_rate,
            streaming_info.channels,
            streaming_info.bit_rate
        );

        Ok(Self {
            rtp_sender,
            opus_encoder,
            direct_streamer: None,
            running: Arc::new(AtomicBool::new(false)),
            audio_config,
            dest_addr,
            streaming_info: streaming_info.clone(),
        })
    }

    /// 流式传输音频文件
    pub async fn stream_file(&mut self,
        file_path: impl AsRef<Path>,
    ) -> Result<()> {
        let file_path = file_path.as_ref();
        info!("开始流式传输文件: {:?}", file_path);

        // 设置运行状态
        self.running.store(true, Ordering::SeqCst);

        // 创建音频解码器
        let mut decoder = AudioDecoder::from_path(file_path)?;

        info!(
            "音频文件信息: {}Hz, {} 声道",
            decoder.sample_rate(),
            decoder.channels()
        );

        // 准备缓冲区
        let mut frame_buffer: Vec<i16> = Vec::with_capacity(self.opus_encoder.frame_size());

        // 创建定时器以维持正确的播放速率
        let frame_duration = Duration::from_millis(20); // 20ms 每帧
        let mut interval = interval(frame_duration);

        // 主循环：读取、编码、发送
        while self.running.load(Ordering::SeqCst) {
            // 尝试读取下一帧
            match decoder.next_frame()? {
                Some(samples) => {
                    // 将样本添加到缓冲区
                    frame_buffer.extend_from_slice(&samples);

                    // 处理完整的帧
                    while frame_buffer.len() >= self.opus_encoder.frame_size() {
                        let frame_to_encode: Vec<i16> = frame_buffer
                            .drain(..self.opus_encoder.frame_size())
                            .collect();

                        // 编码为 Opus
                        match self.opus_encoder.encode(&frame_to_encode) {
                            Ok(opus_data) => {
                                // 发送 RTP 包
                                if let Err(e) = self.rtp_sender.send_opus_frame(&opus_data) {
                                    warn!("发送 RTP 包失败: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Opus 编码失败: {}", e);
                            }
                        }

                        // 等待下一帧时间
                        interval.tick().await;
                    }
                }
                None => {
                    // 文件结束
                    debug!("音频文件解码完成");
                    break;
                }
            }
        }

        // 处理剩余数据
        if !frame_buffer.is_empty() {
            debug!("处理剩余 {} 个样本", frame_buffer.len());
            match self.opus_encoder.encode_final(&frame_buffer) {
                Ok(opus_data) => {
                    let _ = self.rtp_sender.send_opus_frame(&opus_data);
                }
                Err(e) => {
                    warn!("最终帧编码失败: {}", e);
                }
            }
        }

        info!("音频流式传输完成");
        Ok(())
    }

    /// 停止流式传输
    pub fn stop(&mut self) {
        info!("停止音频流");
        self.running.store(false, Ordering::SeqCst);
        
        if let Some(ref mut streamer) = self.direct_streamer {
            streamer.stop();
        }
    }
    
    /// 流式传输 URL 音频 (使用 FFmpeg 直接推流)
    pub fn stream_url(&mut self, url: &str) -> Result<()> {
        info!("开始从 URL 流式传输: {}", url);
        
        if self.direct_streamer.is_none() {
            let config = StreamerConfig::from(&self.streaming_info);
            self.direct_streamer = Some(FFmpegDirectStreamer::new(config)?);
        }
        
        let streamer = self.direct_streamer.as_mut().unwrap();
        streamer.start_stream_url(
            url,
            &self.streaming_info.ip,
            self.streaming_info.port,
            self.streaming_info.rtcp_port,
        )?;
        
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
    
    /// 等待推流结束
    pub fn wait(&mut self) -> Result<()> {
        if let Some(ref mut streamer) = self.direct_streamer {
            streamer.wait()?;
        }
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 获取统计信息
    pub fn stats(&self) -> RtpStats {
        self.rtp_sender.stats()
    }
}

