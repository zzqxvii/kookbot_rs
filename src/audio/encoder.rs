use crate::error::{BotError, Result};
use opus::{Application, Channels, Encoder as OpusEncoderInner};
use tracing::{debug, error, info, warn};

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

/// Opus 编码器
pub struct OpusEncoder {
    encoder: OpusEncoderInner,
    config: OpusConfig,
    frame_size: usize,
}

impl OpusEncoder {
    /// 创建新的 Opus 编码器
    pub fn new(config: OpusConfig) -> Result<Self> {
        let channels = match config.channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => {
                return Err(BotError::OpusError(format!(
                    "不支持的声道数: {}",
                    config.channels
                )))
            }
        };

        let application = match config.application {
            OpusApplication::Voip => Application::Voip,
            OpusApplication::Audio => Application::Audio,
            OpusApplication::RestrictedLowdelay => Application::RestrictedLowdelay,
        };

        let mut encoder = OpusEncoderInner::new(config.sample_rate, channels, application)
            .map_err(|e| {
                BotError::OpusError(format!("创建 Opus 编码器失败: {:?}", e))
            })?;

        // 设置比特率
        encoder
            .set_bitrate(opus::Bitrate::Bits(config.bit_rate))
            .map_err(|e| {
                BotError::OpusError(format!("设置比特率失败: {:?}", e))
            })?;

        // 计算帧大小 (20ms)
        let frame_size = (config.sample_rate as usize * 20) / 1000 * config.channels;

        info!(
            "Opus 编码器创建成功: {}Hz, {} 声道, {}bps, 帧大小: {}",
            config.sample_rate, config.channels, config.bit_rate, frame_size
        );

        Ok(Self {
            encoder,
            config,
            frame_size,
        })
    }

    /// 编码一帧 PCM 数据
    ///
    /// # 参数
    /// * `pcm` - PCM 样本 (i16)，必须是 config.frame_size 大小
    ///
    /// # 返回
    /// * 编码后的 Opus 数据
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        if pcm.len() != self.frame_size {
            return Err(BotError::OpusError(format!(
                "PCM 数据长度不匹配: 期望 {}, 实际 {}",
                self.frame_size, pcm.len()
            )));
        }

        // 最大 Opus 包大小
        let mut output = vec![0u8; 1275];

        let len = self
            .encoder
            .encode(pcm, &mut output)
            .map_err(|e| BotError::OpusError(format!("编码失败: {:?}", e)))?;

        output.truncate(len);
        Ok(output)
    }

    /// 编码剩余数据（用于文件末尾）
    pub fn encode_final(&mut self,
        pcm: &[i16],
    ) -> Result<Vec<u8>> {
        // 填充到完整帧
        let padding = self.frame_size - pcm.len();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_encoder_creation() {
        let config = OpusConfig::default();
        let encoder = OpusEncoder::new(config);
        assert!(encoder.is_ok());
    }
}