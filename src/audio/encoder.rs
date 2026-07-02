//! Opus 编码器 — **STUB 实现**
//!
//! 此模块为模拟 Opus 编码器，生产代码不使用。
//! 实际 Opus 编码由 `FFmpegOpusEncoder`（`ffmpeg_encoder.rs`）完成。
//! 保留此文件以便将来实现纯 Rust Opus 编码（如 `opus` crate）。

use crate::core::error::{BotError, Result};
use tracing::{info, trace};

/// Opus 编码器配置
#[derive(Debug, Clone, Copy)]
pub struct OpusConfig {
    /// 采样率 (Hz)
    pub sample_rate: u32,
    /// 声道数
    pub channels: usize,
    /// 比特率 (bps)
    pub bit_rate: i32,
    /// 应用类型
    pub application: OpusApplication,
}

#[derive(Debug, Clone, Copy)]
pub enum OpusApplication {
    /// 语音 (低延迟)
    Voip,
    /// 音频 (高音质)
    Audio,
    /// 低延迟限制 (LL)
    RestrictedLowdelay,
}

impl Default for OpusConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            bit_rate: 64000,
            application: OpusApplication::Audio,
        }
    }
}

/// Opus 编码器（模拟实现）
///
/// 注意：此版本不包含真正的 Opus 编码，因为 opus crate 需要系统库。
/// 真正的 Opus 编码需要：
/// - Windows: vcpkg install opus
/// - Linux: apt-get install libopus-dev
/// - macOS: brew install opus
///
/// 或者使用预编译的 opus 动态库。
pub struct OpusEncoder {
    config: OpusConfig,
    frame_size: usize,
}

impl OpusEncoder {
    /// 创建新的 Opus 编码器
    pub fn new(config: OpusConfig) -> Result<Self> {
        if config.channels != 1 && config.channels != 2 {
            return Err(BotError::OpusError(format!(
                "不支持的声道数: {}",
                config.channels
            )));
        }

        // 计算帧大小 (20ms)
        let frame_size = (config.sample_rate as usize * 20) / 1000 * config.channels;

        info!(
            "Opus 编码器创建成功: {}Hz, {} 声道, {}bps, 帧大小: {}",
            config.sample_rate, config.channels, config.bit_rate, frame_size
        );

        Ok(Self {
            config,
            frame_size,
        })
    }

    /// 编码一帧 PCM 数据（模拟实现）
    ///
    /// 注意：此实现返回模拟的 Opus 数据，不包含真正的 Opus 编码。
    /// 要获得真正的 Opus 编码，需要：
    /// 1. 安装 opus 库并启用 opus crate
    /// 2. 或者使用外部 Opus 编码器（如 FFmpeg）
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        if pcm.len() != self.frame_size {
            return Err(BotError::OpusError(format!(
                "PCM 数据长度不匹配: 期望 {}, 实际 {}",
                self.frame_size, pcm.len()
            )));
        }

        // 模拟 Opus 编码结果
        // 在真正的实现中，这里会调用 opus 库进行编码
        // 返回值应该是 Opus 编码后的数据

        // 创建一个模拟的 Opus 帧（仅用于测试）
        // 真正的 Opus 帧会更复杂
        let mut output = vec![0x80u8]; // Opus 帧头（模拟）

        // 添加一些模拟的编码数据
        // 实际情况下，这里会是真正的 Opus 编码数据
        let encoded_size = (self.config.bit_rate as usize * 20) / (8 * 1000); // 20ms 的比特数
        output.extend(vec![0u8; encoded_size.max(10)]);

        trace!("模拟编码 {} 样本为 {} 字节 Opus 数据", pcm.len(), output.len());
        Ok(output)
    }

    /// 编码剩余数据（用于文件末尾）
    pub fn encode_final(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        // 填充到完整帧
        let _padding = self.frame_size.saturating_sub(pcm.len());
        let mut full_frame = Vec::with_capacity(self.frame_size);
        full_frame.extend_from_slice(pcm);
        full_frame.resize(self.frame_size, 0);

        self.encode(&full_frame)
    }

    /// 获取帧大小
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// 获取采样率
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// 获取声道数
    pub fn channels(&self) -> usize {
        self.config.channels
    }

    /// 检查是否为模拟实现
    pub fn is_simulated(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_encoder_creation() {
        let config = OpusConfig::default();
        let encoder = OpusEncoder::new(config);
        assert!(encoder.is_ok());

        let encoder = encoder.unwrap();
        assert!(encoder.is_simulated());
        assert_eq!(encoder.sample_rate(), 48000);
        assert_eq!(encoder.channels(), 2);
    }

    #[test]
    fn test_opus_encode() {
        let config = OpusConfig::default();
        let mut encoder = OpusEncoder::new(config).unwrap();

        // 创建测试数据（20ms @ 48kHz 立体声 = 1920 样本 = 3840 字节）
        let frame_size = encoder.frame_size();
        let pcm: Vec<i16> = vec![0i16; frame_size];

        let result = encoder.encode(&pcm);
        assert!(result.is_ok());

        let opus_data = result.unwrap();
        assert!(!opus_data.is_empty());
    }

    #[test]
    fn test_opus_encode_wrong_size() {
        let config = OpusConfig::default();
        let mut encoder = OpusEncoder::new(config).unwrap();

        // 错误的数据大小
        let wrong_pcm = vec![0i16; 100];
        let result = encoder.encode(&wrong_pcm);
        assert!(result.is_err());
    }
}