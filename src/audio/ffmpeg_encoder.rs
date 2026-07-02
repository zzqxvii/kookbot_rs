use crate::core::error::{BotError, Result};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use tracing::{debug, info, trace, warn};

/// FFmpeg Opus 编码器配置
#[derive(Debug, Clone)]
pub struct FFmpegOpusConfig {
    /// 采样率 (Hz)
    pub sample_rate: u32,
    /// 声道数
    pub channels: usize,
    /// 比特率 (bps)
    pub bit_rate: i32,
    /// 帧大小 (毫秒)
    pub frame_duration_ms: u32,
    /// FFmpeg 可执行文件路径
    pub ffmpeg_path: Option<String>,
}

impl Default for FFmpegOpusConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            bit_rate: 64000,
            frame_duration_ms: 20,
            ffmpeg_path: None,
        }
    }
}

/// FFmpeg Opus 编码器
///
/// 使用 FFmpeg 进程进行 Opus 编码，无需 opus 系统库。
///
/// 使用方法：
/// 1. 安装 FFmpeg (https://ffmpeg.org/download.html)
/// 2. 确保 FFmpeg 在 PATH 中，或指定 ffmpeg_path
pub struct FFmpegOpusEncoder {
    config: FFmpegOpusConfig,
    frame_size: usize,
    ffmpeg_process: Option<Child>,
}

impl FFmpegOpusEncoder {
    /// 创建新的 FFmpeg Opus 编码器
    pub fn new(config: FFmpegOpusConfig) -> Result<Self> {
        if config.channels != 1 && config.channels != 2 {
            return Err(BotError::OpusError(format!(
                "不支持的声道数: {}",
                config.channels
            )));
        }

        // 检查 FFmpeg 是否可用
        let ffmpeg_path = config.ffmpeg_path.clone()
            .unwrap_or_else(|| "ffmpeg".to_string());

        match Command::new(&ffmpeg_path)
            .arg("-version")
            .output() {
            Ok(output) => {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout);
                    let first_line = version.lines().next().unwrap_or("Unknown");
                    info!("FFmpeg 版本: {}", first_line);
                } else {
                    warn!("FFmpeg 检查失败，状态码: {:?}", output.status);
                }
            }
            Err(e) => {
                return Err(BotError::ConfigError(format!(
                    "无法启动 FFmpeg ({}): {}。请确保 FFmpeg 已安装并在 PATH 中，或配置 ffmpeg_path",
                    ffmpeg_path, e
                )));
            }
        }

        // 计算帧大小
        let frame_size = (config.sample_rate as usize * config.frame_duration_ms as usize)
            / 1000 * config.channels;

        info!(
            "FFmpeg Opus 编码器创建成功: {}Hz, {} 声道, {}bps, 帧大小: {}",
            config.sample_rate, config.channels, config.bit_rate, frame_size
        );

        Ok(Self {
            config,
            frame_size,
            ffmpeg_process: None,
        })
    }

    /// 启动 FFmpeg 编码进程
    fn start_ffmpeg_process(&mut self) -> Result<&mut Child> {
        if self.ffmpeg_process.is_none() {
            let ffmpeg_path = self.config.ffmpeg_path.clone()
                .unwrap_or_else(|| "ffmpeg".to_string());

            let child = Command::new(&ffmpeg_path)
                .args([
                    "-f", "s16le",           // 输入格式: 16-bit PCM
                    "-ar", &self.config.sample_rate.to_string(), // 采样率
                    "-ac", &self.config.channels.to_string(),   // 声道数
                    "-i", "-",              // 从 stdin 读取
                    "-c:a", "libopus",      // 编码器: Opus
                    "-b:a", &format!("{}k", self.config.bit_rate / 1000), // 比特率
                    "-vbr", "on",           // 可变比特率
                    "-application", "audio", // 音频应用
                    "-f", "opus",           // 输出格式
                    "-",                    // 输出到 stdout
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| BotError::IoError(e))?;

            debug!("FFmpeg 进程已启动 (PID: {:?})", child.id());
            self.ffmpeg_process = Some(child);
        }

        self.ffmpeg_process.as_mut()
            .ok_or_else(|| BotError::OpusError("FFmpeg 进程未启动".to_string()))
    }

    /// 编码一帧 PCM 数据
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        if pcm.len() != self.frame_size {
            return Err(BotError::OpusError(format!(
                "PCM 数据长度不匹配: 期望 {}, 实际 {}",
                self.frame_size, pcm.len()
            )));
        }

        // 启动 FFmpeg 进程
        let child = self.start_ffmpeg_process()?;

        // 将 PCM 数据写入 FFmpeg stdin
        let stdin = child.stdin.as_mut()
            .ok_or_else(|| BotError::IoError(
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "无法获取 stdin")
            ))?;

        // 将 PCM 数据转换为字节
        let pcm_bytes: Vec<u8> = pcm.iter()
            .flat_map(|&sample| sample.to_le_bytes())
            .collect();

        stdin.write_all(&pcm_bytes)
            .map_err(|e| BotError::IoError(e))?;

        // 读取 FFmpeg stdout 的输出（Opus 数据）
        let stdout = child.stdout.as_mut()
            .ok_or_else(|| BotError::IoError(
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "无法获取 stdout")
            ))?;

        use std::io::Read;

        let mut opus_data = Vec::new();
        stdout.read_to_end(&mut opus_data)
            .map_err(|e| BotError::IoError(e))?;

        trace!("编码 {} 样本为 {} 字节 Opus 数据", pcm.len(), opus_data.len());
        Ok(opus_data)
    }

    /// 编码最终的 PCM 数据（剩余的数据）
    /// 对于 FFmpeg 编码器，直接调用 encode 方法即可
    pub fn encode_final(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }

        // 复用 encode 方法进行编码
        // 注意：如果数据长度不足一帧，FFmpeg 可能无法正确处理
        // 这里简单起见，直接调用 encode
        // 实际应用中可能需要填充到完整帧大小
        self.encode(pcm)
    }

    /// 停止 FFmpeg 进程
    pub fn stop(&mut self) {
        if let Some(mut child) = self.ffmpeg_process.take() {
            debug!("正在停止 FFmpeg 进程 (PID: {:?})", child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
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

impl Drop for FFmpegOpusEncoder {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffmpeg_opus_config_default() {
        let config = FFmpegOpusConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channels, 2);
        assert_eq!(config.bit_rate, 64000);
    }
}