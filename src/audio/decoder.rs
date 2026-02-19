use crate::core::error::{BotError, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::Signal;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tracing::{info, warn};

/// 音频解码器
pub struct AudioDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: usize,
}

impl AudioDecoder {
    /// 从文件路径创建解码器
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        info!("正在打开音频文件: {:?}", path);

        let file = File::open(path).map_err(|e| {
            BotError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("无法打开音频文件: {}", e),
            ))
        })?;

        // 创建媒体源流
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // 创建提示以帮助识别格式
        let mut hint = Hint::new();
        if let Some(extension) = path.extension() {
            hint.with_extension(extension.to_str().unwrap_or(""));
        }

        // 使用默认选项
        let format_opts: FormatOptions = Default::default();
        let metadata_opts: MetadataOptions = Default::default();
        let decoder_opts: DecoderOptions = Default::default();

        // 探测格式
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| BotError::AudioDecodeError(format!("无法识别音频格式: {:?}", e)))?;

        let mut format = probed.format;

        // 找到第一个音频轨道
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| BotError::AudioDecodeError("未找到音频轨道".to_string()))?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        // 创建解码器
        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &decoder_opts)
            .map_err(|e| BotError::AudioDecodeError(format!("创建解码器失败: {:?}", e)))?;

        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let channels = codec_params.channels.map(|c| c.count()).unwrap_or(2);

        info!(
            "音频解码器创建成功: {}Hz, {} 声道, 轨道 ID: {}",
            sample_rate, channels, track_id
        );

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
        })
    }

    /// 获取采样率
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 获取声道数
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// 读取下一帧音频数据（返回 i16 样本）
    pub fn next_frame(&mut self,
    ) -> Result<Option<Vec<i16>>> {
        loop {
            // 获取下一个数据包
            let packet = match self.format.next_packet() {
                Ok(pkt) => pkt,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None); // 文件结束
                }
                Err(e) => {
                    return Err(BotError::AudioDecodeError(format!(
                        "读取数据包失败: {:?}",
                        e
                    )));
                }
            };

            // 只处理目标轨道的数据包
            if packet.track_id() != self.track_id {
                continue;
            }

            // 解码音频数据
            match self.decoder.decode(&packet) {
                Ok(audio_buf) => {
                    // 转换为 i16 样本
                    let samples = Self::convert_to_i16(&audio_buf);
                    return Ok(Some(samples));
                }
                Err(symphonia::core::errors::Error::DecodeError(e)) => {
                    warn!("解码错误，跳过此帧: {}", e);
                    continue;
                }
                Err(e) => {
                    return Err(BotError::AudioDecodeError(format!(
                        "解码失败: {:?}",
                        e
                    )));
                }
            }
        }
    }

    /// 将音频缓冲区转换为 i16 样本
    fn convert_to_i16(
        audio_buf: &symphonia::core::audio::AudioBufferRef,
    ) -> Vec<i16> {
        use symphonia::core::audio::AudioBufferRef;

        // 获取声道数和帧数
        let channels = audio_buf.spec().channels.count();
        let frames = audio_buf.frames();

        let mut samples = Vec::with_capacity(frames * channels);

        // 根据样本类型进行转换
        match audio_buf {
            AudioBufferRef::S16(buf) => {
                for frame in 0..frames {
                    for ch in 0..channels {
                        samples.push(buf.chan(ch)[frame]);
                    }
                }
            }
            AudioBufferRef::S32(buf) => {
                for frame in 0..frames {
                    for ch in 0..channels {
                        let sample = buf.chan(ch)[frame];
                        // 将 32 位转换为 16 位
                        samples.push((sample >> 16) as i16);
                    }
                }
            }
            AudioBufferRef::F32(buf) => {
                for frame in 0..frames {
                    for ch in 0..channels {
                        let sample = buf.chan(ch)[frame];
                        // 将浮点转换为 16 位有符号整数
                        let scaled = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                        samples.push(scaled);
                    }
                }
            }
            AudioBufferRef::F64(buf) => {
                for frame in 0..frames {
                    for ch in 0..channels {
                        let sample = buf.chan(ch)[frame];
                        let scaled = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                        samples.push(scaled);
                    }
                }
            }
            _ => {
                // 对于其他格式，使用 symphonia 的转换
                let mut sample_buf =
                    symphonia::core::audio::SampleBuffer::<i16>::new(
                        frames as u64,
                        *audio_buf.spec(),
                    );
                sample_buf.copy_interleaved_ref(audio_buf.clone());
                samples.extend_from_slice(sample_buf.samples());
            }
        }

        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 这些测试需要实际的音频文件
    // 在 CI 环境中可以跳过或使用 mock
}