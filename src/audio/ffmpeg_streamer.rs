use crate::common::play_state::PlayState;
use crate::core::error::{BotError, Result};
use crate::player::VoiceStreamingInfo;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// FFmpeg 直接推流器
///
/// 使用 FFmpeg 从 URL/文件/stdin 读取音频，编码为 Opus 并推送到 RTP。
/// 子进程管理使用 `std::process::Command`；stdin 推流模式通过
/// `tokio::process::ChildStdin::from_std()` 转换为异步 I/O。
pub struct FFmpegDirectStreamer {
    config: StreamerConfig,
    process: Option<Child>,
    running: Arc<AtomicBool>,
    play_state: Arc<PlayState>,
    /// concat 播放列表临时文件 (stop/wait 时清理)
    concat_file: Option<std::path::PathBuf>,
    /// stderr 读取任务句柄 (stop/wait 时 join)
    stderr_threads: Vec<tokio::task::JoinHandle<()>>,
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
                    "FFmpeg 未找到或无法执行: {}. 请确保 FFmpeg 已安装并在 PATH 中",
                    e
                ))
            })?;

        if !output.status.success() {
            return Err(BotError::ConfigError(
                "FFmpeg 执行失败，请检查 FFmpeg 安装".into(),
            ));
        }
        Ok(())
    }

    // ── private helpers ──────────────────────────────────────────

    /// 构建 FFmpeg RTP 推流的公共参数部分。
    ///
    /// 输入参数（URL/pipe/concat）由调用者通过 `input_args` 提供。
    fn build_rtp_command(&self, dest_ip: &str, dest_port: u16, rtcp_port: u16, input_args: &[&str]) -> Command {
        let rtp_url = format!("rtp://{}:{}?rtcpport={}", dest_ip, dest_port, rtcp_port);
        let bit_rate_k = self.config.bit_rate / 1000;
        let volume = self.config.volume;
        let ssrc = self.config.ssrc;
        let pt = self.config.pt;

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-re", "-loglevel", "warning", "-hide_banner"]);
        cmd.args(input_args);
        cmd.args([
            "-map", "0:a:0",
            "-acodec", "libopus",
            "-b:a", &format!("{}k", bit_rate_k),
            "-vbr", "on",
            "-compression_level", "10",
            "-filter:a", &format!("volume={}", volume),
            "-ac", "2",
            "-ar", "48000",
            "-f", "tee",
            &format!("[select=a:f=rtp:ssrc={}:payload_type={}]{}", ssrc, pt, rtp_url),
        ]);
        cmd
    }

    // ── 公共 API ──────────────────────────────────────────────

    /// 从 URL 开始推流（同步 — 子进程 spawn 是非阻塞的）
    pub fn start_stream_url(
        &mut self,
        url: &str,
        dest_ip: &str,
        dest_port: u16,
        rtcp_port: u16,
    ) -> Result<()> {
        if self.running.load(Ordering::Acquire) {
            self.stop();
        }

        info!("🎵 开始播放: {}", url);

        let input_args = ["-i", url];
        let mut child = self.build_rtp_command(dest_ip, dest_port, rtcp_port, &input_args)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BotError::IoError(e))?;

        let pid = child.id();
        info!("FFmpeg 进程已启动 (PID: {:?})", pid);
        self.play_state.set_playing(pid);

        self.spawn_stderr_reader(&mut child);
        self.running.store(true, Ordering::Release);
        self.process = Some(child);
        Ok(())
    }

    /// 从 stdin 推流 (pipe 模式)
    ///
    /// FFmpeg 从 stdin 读取 MP3 数据，持续编码并推流。
    /// 返回 `tokio::process::ChildStdin`（异步写入端），调用者使用
    /// `AsyncWriteExt::write_all` 逐首喂入歌曲数据。
    /// stdin 关闭时 FFmpeg 自然退出。
    pub fn start_stream_stdin(
        &mut self,
        dest_ip: &str,
        dest_port: u16,
        rtcp_port: u16,
    ) -> Result<std::process::ChildStdin> {
        if self.running.load(Ordering::Acquire) {
            self.stop();
        }

        info!("🎵 开始 stdin pipe 推流");

        let input_args = ["-f", "mp3", "-i", "pipe:0"];
        let mut child = self.build_rtp_command(dest_ip, dest_port, rtcp_port, &input_args)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BotError::IoError(e))?;

        let pid = child.id();
        info!("FFmpeg stdin 进程已启动 (PID: {:?})", pid);
        self.play_state.set_playing(pid);

        self.spawn_stderr_reader(&mut child);

        let stdin = child.stdin.take().expect("stdin should be piped");

        self.running.store(true, Ordering::Release);
        self.process = Some(child);
        Ok(stdin)
    }

    /// 从文件列表开始推流 (concat demuxer 模式)
    pub fn start_stream_files(
        &mut self,
        file_paths: &[String],
        dest_ip: &str,
        dest_port: u16,
        rtcp_port: u16,
    ) -> Result<()> {
        if self.running.load(Ordering::Acquire) {
            self.stop();
        }

        if file_paths.is_empty() {
            return Err(BotError::ConfigError("文件列表为空".into()));
        }

        // 清理先前残留的 concat 文件
        if let Some(path) = &self.concat_file {
            let _ = std::fs::remove_file(path);
        }

        let concat_path = std::env::temp_dir().join(format!(
            "kook_concat_{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));

        let mut f = std::fs::File::create(&concat_path).map_err(|e| BotError::IoError(e))?;
        use std::io::Write;
        for path in file_paths {
            let normalized = path.replace('\\', "/");
            writeln!(f, "file '{}'", normalized).map_err(|e| BotError::IoError(e))?;
        }
        f.flush().map_err(|e| BotError::IoError(e))?;
        drop(f);
        self.concat_file = Some(concat_path.clone());

        info!("🎵 开始 concat 播放: {} 个文件", file_paths.len());
        debug!("Concat 文件: {:?}", concat_path);

        let concat_path_str = concat_path.to_string_lossy().to_string();
        let input_args = ["-f", "concat", "-safe", "0", "-i", &concat_path_str];
        let mut child = self.build_rtp_command(dest_ip, dest_port, rtcp_port, &input_args)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BotError::IoError(e))?;

        let pid = child.id();
        info!("FFmpeg concat 进程已启动 (PID: {:?})", pid);
        self.play_state.set_playing(pid);

        self.spawn_stderr_reader(&mut child);
        self.running.store(true, Ordering::Release);
        self.process = Some(child);
        Ok(())
    }

    /// 停止推流（同步）
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
            self.play_state.set_stopped();
            info!("⏹️ 播放已停止");
        }
        self.join_stderr_threads();
        self.cleanup_concat_file();
    }

    /// 等待推流结束（异步 — 通过 spawn_blocking 等待子进程退出）
    pub async fn wait(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            let result = tokio::task::spawn_blocking(move || child.wait())
                .await
                .map_err(|e| BotError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            result.map_err(|e| BotError::IoError(e))?;
            self.running.store(false, Ordering::Release);
        }
        self.join_stderr_threads();
        self.cleanup_concat_file();
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    // ── private helpers ──────────────────────────────────────────

    /// 在 `spawn_blocking` 中读取 FFmpeg stderr。
    ///
    /// stderr 读取是低吞吐量的诊断 I/O，阻塞式逐行读取不会影响 tokio runtime。
    fn spawn_stderr_reader(&mut self, child: &mut Child) {
        let stderr = match child.stderr.take() {
            Some(s) => s,
            None => {
                warn!("stderr 不可用（可能已被取走）");
                return;
            }
        };

        let running = self.running.clone();

        let handle = tokio::task::spawn_blocking(move || {
            use std::io::{BufRead, BufReader};
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if !running.load(Ordering::Acquire) {
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

        self.stderr_threads.push(handle);
    }

    /// Join 所有 stderr 读取任务
    fn join_stderr_threads(&mut self) {
        if self.stderr_threads.is_empty() {
            return;
        }
        let handles: Vec<_> = self.stderr_threads.drain(..).collect();
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                for h in handles {
                    let _ = h.await;
                }
            });
        });
    }

    /// 幂等清理 concat 临时文件
    fn cleanup_concat_file(&mut self) {
        if let Some(path) = &self.concat_file {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for FFmpegDirectStreamer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.cleanup_concat_file();
    }
}
