use crate::common::play_state::PlayState;
use crate::core::error::{BotError, Result};
use crate::player::VoiceStreamingInfo;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// FFmpeg 直接推流器
///
/// 使用 FFmpeg 从 URL 直接读取音频，编码为 Opus 并推送到 RTP
/// 这比解码->编码->发送的方式更高效
pub struct FFmpegDirectStreamer {
    config: StreamerConfig,
    process: Option<Child>,
    running: Arc<AtomicBool>,
    play_state: Arc<PlayState>,
    /// concat 播放列表临时文件 (wait 后清理)
    concat_file: Option<std::path::PathBuf>,
    /// stderr 读取线程句柄 (stop 时 join)
    stderr_threads: Vec<std::thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct StreamerConfig {
    pub ssrc: u32,
    pub pt: u8,
    pub bit_rate: i32,
    pub sample_rate: u32,
    pub channels: usize,
    pub volume: f32,
}

impl From<&VoiceStreamingInfo> for StreamerConfig {
    fn from(info: &VoiceStreamingInfo) -> Self {
        Self {
            ssrc: info.ssrc,
            pt: info.pt,
            bit_rate: info.bit_rate,
            sample_rate: info.sample_rate,
            channels: info.channels,
            volume: 0.5,
        }
    }
}
impl FFmpegDirectStreamer {
    pub fn new(config: StreamerConfig, play_state: Arc<PlayState>) -> Result<Self> {
        Self::check_ffmpeg()?;

        info!(
            "FFmpeg 直接推流器创建成功: SSRC={}, PT={}, {}bps",
            config.ssrc, config.pt, config.bit_rate
        );
        Ok(Self {
            config,
            process: None,
            running: Arc::new(AtomicBool::new(false)),
            play_state,
            concat_file: None,
            stderr_threads: Vec::new(),
        })
    }

    fn check_ffmpeg() -> Result<()> {
        let output = Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map_err(|e| {
                BotError::ConfigError(format!(
                    "无法启动 FFmpeg: {}。请确保 FFmpeg 已安装并在 PATH 中",
                    e
                ))
            })?;

        if !output.status.success() {
            return Err(BotError::ConfigError("FFmpeg 版本检查失败".into()));
        }

        let version = String::from_utf8_lossy(&output.stdout);
        let first_line = version.lines().next().unwrap_or("Unknown");
        debug!("FFmpeg 版本: {}", first_line);

        Ok(())
    }

    /// 从 URL 开始推流
    ///
    /// # Arguments
    /// * `url` - 音频 URL (HTTP/HTTPS)
    /// * `dest_ip` - 目标 IP
    /// * `dest_port` - 目标端口
    /// * `rtcp_port` - RTCP 端口
    pub fn start_stream_url(
        &mut self,
        url: &str,
        dest_ip: &str,
        dest_port: u16,
        rtcp_port: u16,
    ) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            self.stop();
        }

        let rtp_url = format!("rtp://{}:{}?rtcpport={}", dest_ip, dest_port, rtcp_port);

        let bit_rate_k = self.config.bit_rate / 1000;
        let volume = self.config.volume;
        let ssrc = self.config.ssrc;
        let pt = self.config.pt;

        info!("🎵 开始播放: {}", url);

        let mut child = Command::new("ffmpeg")
            .args([
                "-re",
                "-loglevel",
                "warning",
                "-hide_banner",
                "-i",
                url,
                "-map",
                "0:a:0",
                "-acodec",
                "libopus",
                "-b:a",
                &format!("{}k", bit_rate_k),
                "-vbr",
                "on",
                "-compression_level",
                "10",
                "-filter:a",
                &format!("volume={}", volume),
                "-ac",
                "2",
                "-ar",
                "48000",
                "-f",
                "tee",
                &format!(
                    "[select=a:f=rtp:ssrc={}:payload_type={}]{}",
                    ssrc, pt, rtp_url
                ),
            ])
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BotError::IoError(e))?;

        let pid = child.id();
        info!("FFmpeg 进程已启动 (PID: {:?})", pid);
        self.play_state.set_playing(pid);

        let stderr = child.stderr.take().expect("stderr should be piped");
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        let stderr_handle = std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(line) = line {
                    let line_lower = line.to_lowercase();
                    // 只显示错误和警告，其他都忽略
                    if line_lower.contains("error") {
                        error!("[FFmpeg] {}", line);
                    } else if line_lower.contains("warning") {
                        warn!("[FFmpeg] {}", line);
                    }
                    // 进度信息 (size=, time=, bitrate=) 和其他日志都不显示
                }
            }
        });
        self.stderr_threads.push(stderr_handle);

        self.process = Some(child);
        Ok(())
    }

    /// 从 stdin 推流 (pipe 模式)
    ///
    /// FFmpeg 从 stdin 读取 MP3 数据，持续编码并推流。
    /// 调用者按顺序向 stdin 写入音频文件，FFmpeg 无缝衔接。
    /// stdin 关闭时 FFmpeg 自然退出。
    ///
    /// 返回 stdin 写入端，供调用者逐首喂入歌曲数据。
    pub fn start_stream_stdin(
        &mut self,
        dest_ip: &str,
        dest_port: u16,
        rtcp_port: u16,
    ) -> Result<std::process::ChildStdin> {
        if self.running.load(Ordering::SeqCst) {
            self.stop();
        }

        let rtp_url = format!("rtp://{}:{}?rtcpport={}", dest_ip, dest_port, rtcp_port);
        let bit_rate_k = self.config.bit_rate / 1000;
        let volume = self.config.volume;
        let ssrc = self.config.ssrc;
        let pt = self.config.pt;

        info!("🎵 开始 stdin pipe 推流");

        let mut child = Command::new("ffmpeg")
            .args([
                "-re",
                "-loglevel", "warning",
                "-hide_banner",
                "-f", "mp3",
                "-i", "pipe:0",
                "-map", "0:a:0",
                "-acodec", "libopus",
                "-b:a", &format!("{}k", bit_rate_k),
                "-vbr", "on",
                "-compression_level", "10",
                "-filter:a", &format!("volume={}", volume),
                "-ac", "2",
                "-ar", "48000",
                "-f", "tee",
                &format!(
                    "[select=a:f=rtp:ssrc={}:payload_type={}]{}",
                    ssrc, pt, rtp_url
                ),
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BotError::IoError(e))?;

        let pid = child.id();
        info!("FFmpeg stdin 进程已启动 (PID: {:?})", pid);
        self.play_state.set_playing(pid);

        let stderr = child.stderr.take().expect("stderr should be piped");
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        let stderr_handle = std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(line) = line {
                    let line_lower = line.to_lowercase();
                    if line_lower.contains("error") {
                        error!("[FFmpeg] {}", line);
                    } else if line_lower.contains("warning") {
                        warn!("[FFmpeg] {}", line);
                    }
                }
            }
        });
        self.stderr_threads.push(stderr_handle);

        let stdin = child.stdin.take().expect("stdin should be piped");
        self.process = Some(child);
        Ok(stdin)
    }
    /// 从文件列表开始推流 (concat demuxer 模式)
    ///
    /// 使用 FFmpeg concat demuxer 顺序播放多个音频文件。
    /// 播放列表文件在 wait() 返回后自动清理。
    pub fn start_stream_files(
        &mut self,
        file_paths: &[String],
        dest_ip: &str,
        dest_port: u16,
        rtcp_port: u16,
    ) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            self.stop();
        }

        if file_paths.is_empty() {
            return Err(BotError::ConfigError("文件列表为空".into()));
        }

        // 创建 concat 播放列表文件 (cache 目录，时间戳唯一命名)
        let concat_path = std::env::temp_dir()
            .join(format!("kook_concat_{}.txt", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)));

        let mut f = std::fs::File::create(&concat_path)
            .map_err(|e| BotError::IoError(e))?;
        use std::io::Write;
        for path in file_paths {
            let normalized = path.replace('\\', "/");
            writeln!(f, "file '{}'", normalized)
                .map_err(|e| BotError::IoError(e))?;
        }
        f.flush().map_err(|e| BotError::IoError(e))?;
        drop(f);
        // 先保存 concat 路径以确保 spawn 失败时也能在 stop()/Drop 中清理
        self.concat_file = Some(concat_path.clone());

        let rtp_url = format!("rtp://{}:{}?rtcpport={}", dest_ip, dest_port, rtcp_port);
        let bit_rate_k = self.config.bit_rate / 1000;
        let volume = self.config.volume;
        let ssrc = self.config.ssrc;
        let pt = self.config.pt;

        info!("🎵 开始 concat 播放: {} 个文件", file_paths.len());
        debug!("Concat 文件: {:?}", concat_path);


        let mut child = Command::new("ffmpeg")
            .args([
                "-re",
                "-loglevel", "warning",
                "-hide_banner",
                "-f", "concat",
                "-safe", "0",
                "-i", &concat_path.to_string_lossy(),
                "-map", "0:a:0",
                "-acodec", "libopus",
                "-b:a", &format!("{}k", bit_rate_k),
                "-vbr", "on",
                "-compression_level", "10",
                "-filter:a", &format!("volume={}", volume),
                "-ac", "2",
                "-ar", "48000",
                "-f", "tee",
                &format!(
                    "[select=a:f=rtp:ssrc={}:payload_type={}]{}",
                    ssrc, pt, rtp_url
                ),
            ])
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BotError::IoError(e))?;

        let pid = child.id();
        info!("FFmpeg concat 进程已启动 (PID: {:?})", pid);
        self.play_state.set_playing(pid);

        let stderr = child.stderr.take().expect("stderr should be piped");
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        let stderr_handle = std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(line) = line {
                    let line_lower = line.to_lowercase();
                    if line_lower.contains("error") {
                        error!("[FFmpeg] {}", line);
                    } else if line_lower.contains("warning") {
                        warn!("[FFmpeg] {}", line);
                    }
                }
            }
        });
        self.stderr_threads.push(stderr_handle);

        self.process = Some(child);
        Ok(())
    }

    /// 停止推流
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
            self.play_state.set_stopped();
            info!("⏹️ 播放已停止");
        }
        // 等待所有 stderr 读取线程结束
        for handle in self.stderr_threads.drain(..) {
            let _ = handle.join();
        }
        // 清理 concat 临时文件
        if let Some(path) = self.concat_file.take() {
            let _ = std::fs::remove_file(&path);
        }
    }

    /// 等待推流结束 (自动清理 concat 播放列表文件)
    pub fn wait(&mut self) -> Result<()> {
        if let Some(child) = &mut self.process {
            let _ = child.wait().map_err(|e| BotError::IoError(e))?;
            self.running.store(false, Ordering::SeqCst);
        }
        // 清理 concat 临时文件
        if let Some(path) = self.concat_file.take() {
            let _ = std::fs::remove_file(&path);
            debug!("已清理 concat 文件: {:?}", path);
        }
        Ok(())
    }
}

impl Drop for FFmpegDirectStreamer {
    fn drop(&mut self) {
        self.stop();
    }
}
