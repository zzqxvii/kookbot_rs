use crate::core::error::{BotError, Result};
use std::io::{BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
    LazyLock,
};
use std::thread;
use std::time::Duration;
use tracing::{debug, info, trace, warn};

/// FFmpeg Opus 编码器配置
#[derive(Debug, Clone)]
pub struct FFmpegOpusConfig {
    pub sample_rate: u32,
    pub channels: usize,
    pub bit_rate: i32,
    pub frame_duration_ms: u32,
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

/// FFmpeg Opus 编码器 — 长驻进程管道模式
pub struct FFmpegOpusEncoder {
    config: FFmpegOpusConfig,
    frame_size: usize,
    ffmpeg_child: Option<Child>,
    stdin_writer: Option<std::process::ChildStdin>,
    opus_rx: Option<Receiver<Vec<u8>>>,
    reader_running: Option<Arc<AtomicBool>>,
}


static VERSION_CHECKED: LazyLock<()> = LazyLock::new(|| {
    match Command::new("ffmpeg").arg("-version").output() {
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
            warn!("无法检查 FFmpeg 版本: {}", e);
        }
    }
});
impl FFmpegOpusEncoder {
    pub fn new(config: FFmpegOpusConfig) -> Result<Self> {
        if config.channels != 1 && config.channels != 2 {
            return Err(BotError::OpusError(format!(
                "不支持的声道数: {}", config.channels
            )));
        }
        LazyLock::force(&VERSION_CHECKED);
        let frame_size = (config.sample_rate as usize * config.frame_duration_ms as usize)
            / 1000 * config.channels;
        info!(
            "FFmpeg Opus 编码器创建成功: {}Hz, {}声道, {}bps, 帧大小: {}",
            config.sample_rate, config.channels, config.bit_rate, frame_size
        );
        Ok(Self {
            config,
            frame_size,
            ffmpeg_child: None,
            stdin_writer: None,
            opus_rx: None,
            reader_running: None,
        })
    }

    fn ensure_process_running(&mut self) -> Result<()> {
        if self.ffmpeg_child.is_some() {
            if let Some(ref mut child) = self.ffmpeg_child {
                match child.try_wait() {
                    Ok(None) => return Ok(()),
                    _ => {
                        debug!("FFmpeg 进程已退出，重新启动");
                        self.stop_inner();
                    }
                }
            }
        }
        let ffmpeg_path = self.config.ffmpeg_path.clone().unwrap_or_else(|| "ffmpeg".to_string());
        let mut child = Command::new(&ffmpeg_path)
            .args([
                "-loglevel", "fatal",           // 只输出致命错误
                "-f", "s16le",                  // 输入格式: 16-bit PCM
                "-ar", &self.config.sample_rate.to_string(), // 采样率
                "-ac", &self.config.channels.to_string(),   // 声道数
                "-i", "pipe:0",                 // 从 stdin 读取 PCM
                "-c:a", "libopus",              // 音频编码器: Opus
                "-b:a", &format!("{}k", self.config.bit_rate / 1000), // 比特率
                "-vbr", "on",                   // 可变比特率
                "-application", "audio",        // 编码器应用模式: audio
                "-f", "opus",                   // 输出格式: 原始 Opus 流
                "pipe:1",                       // 输出到 stdout
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| BotError::IoError(e))?;


        let stdin_writer = child.stdin.take()
            .ok_or_else(|| BotError::OpusError("无法获取 stdin".to_string()))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| BotError::OpusError("无法获取 stdout".to_string()))?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = vec![0u8; 4096];
            while running_clone.load(Ordering::Acquire) {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        debug!("FFmpeg stdout reader 错误: {}", e);
                        break;
                    }
                }
            }
            debug!("FFmpeg stdout reader 线程退出");
        });

        self.ffmpeg_child = Some(child);
        self.stdin_writer = Some(stdin_writer);
        self.opus_rx = Some(rx);
        self.reader_running = Some(running);
        info!("FFmpeg 长驻进程已启动 (PID: {:?})", self.ffmpeg_child.as_ref().map(|c| c.id()));
        Ok(())
    }

    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        if pcm.len() != self.frame_size {
            return Err(BotError::OpusError(format!(
                "PCM 数据长度不匹配: 期望 {}, 实际 {}",
                self.frame_size, pcm.len()
            )));
        }
        self.ensure_process_running()?;
        let stdin = self.stdin_writer.as_mut()
            .ok_or_else(|| BotError::OpusError("stdin 不可用".to_string()))?;

        let pcm_bytes: Vec<u8> = pcm.iter().flat_map(|&s| s.to_le_bytes()).collect();
        stdin.write_all(&pcm_bytes).map_err(|e| BotError::IoError(e))?;
        stdin.flush().map_err(|e| BotError::IoError(e))?;

        let timeout_ms = self.config.frame_duration_ms + 100;
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms as u64);
        let rx = self.opus_rx.as_ref()
            .ok_or_else(|| BotError::OpusError("opus 接收器不可用".to_string()))?;

        let mut opus_data = Vec::new();
        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(chunk) => opus_data.extend(chunk),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(BotError::OpusError("FFmpeg 进程已断开".to_string()));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                match rx.recv_timeout(remaining) {
                    Ok(chunk) => opus_data.extend(chunk),
                    Err(_) => warn!("FFmpeg Opus 编码超时（{}ms）", timeout_ms),
                }
            }
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(2)) {
                Ok(chunk) => opus_data.extend(chunk),
                Err(_) => break,
            }
        }
        trace!("编码 {} 样本为 {} 字节 Opus 数据", pcm.len(), opus_data.len());
        Ok(opus_data)
    }

    pub fn encode_final(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
        let result = self.encode(pcm)?;
        let _ = self.stdin_writer.take();
        if let Some(ref rx) = self.opus_rx {
            let mut remaining = Vec::new();
            loop {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(chunk) => remaining.extend(chunk),
                    Err(_) => break,
                }
            }
            let mut combined = result;
            combined.extend(remaining);
            return Ok(combined);
        }
        Ok(result)
    }

    pub fn stop(&mut self) { self.stop_inner(); }

    fn stop_inner(&mut self) {
        if let Some(running) = self.reader_running.take() {
            running.store(false, Ordering::Release);
        }
        let _ = self.stdin_writer.take();
        let _ = self.opus_rx.take();
        if let Some(mut child) = self.ffmpeg_child.take() {
            debug!("正在停止 FFmpeg 进程 (PID: {:?})", child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn frame_size(&self) -> usize { self.frame_size }
    pub fn sample_rate(&self) -> u32 { self.config.sample_rate }
    pub fn channels(&self) -> usize { self.config.channels }
}

impl Drop for FFmpegOpusEncoder {
    fn drop(&mut self) { self.stop_inner(); }
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
