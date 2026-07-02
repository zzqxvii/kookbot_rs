//! 播放状态管理
//!
//! `PlayState` 封装所有播放相关的可变状态，通过 `Arc<PlayState>` 在线程间共享。
//! 支持多实例部署（每个 Bot 实例拥有独立的 PlayState）。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use tracing::info;

/// 播放状态 — 所有播放控制信号和统计信息
///
/// # 关于 `std::sync::Mutex` 的使用
///
/// `play_msg_id` 和 `channel_id` 使用 `std::sync::Mutex` 而非 `tokio::sync::Mutex`，
/// 原因如下：
/// - 锁持有时间为微秒级（仅 clone/set/take 一个 `Option<String>`）
/// - 从不在 `.await` 点之间持有锁
/// - `tokio::sync::Mutex` 在此场景下反而增加 async 开销
/// - 使用 `.lock().ok()` 模式，锁中毒时静默降级为 None
pub struct PlayState {
    /// 当前播放进程 PID
    pid: AtomicU32,
    /// 是否正在播放
    running: AtomicBool,
    /// 是否请求停止
    stop_requested: AtomicBool,
    /// 是否请求下一首
    next_requested: AtomicBool,
    /// 播放卡片消息 ID（微秒级锁, 从不跨 .await）
    play_msg_id: Mutex<Option<String>>,
    /// 当前语音频道 ID（微秒级锁, 从不跨 .await）
    channel_id: Mutex<Option<String>>,
    /// 已播放歌曲计数
    play_count: AtomicU64,
    /// 播放开始时间 — Unix 时间戳（秒），0 = 未开始
    start_time_secs: AtomicU64,
    /// 是否已记录开始时间
    has_started: AtomicBool,
    /// 当前歌曲总时长（秒），用于进度条
    current_song_duration: AtomicU64,
}

impl Default for PlayState {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayState {
    pub fn new() -> Self {
        Self {
            pid: AtomicU32::new(0),
            running: AtomicBool::new(false),
            stop_requested: AtomicBool::new(false),
            next_requested: AtomicBool::new(false),
            play_msg_id: Mutex::new(None),
            channel_id: Mutex::new(None),
            play_count: AtomicU64::new(0),
            start_time_secs: AtomicU64::new(0),
            has_started: AtomicBool::new(false),
            current_song_duration: AtomicU64::new(0),
        }
    }

    // ── 播放控制 ──

    pub fn set_playing(&self, pid: u32) {
        self.pid.store(pid, Ordering::Release);
        self.running.store(true, Ordering::Release);
        self.stop_requested.store(false, Ordering::Release);
        self.next_requested.store(false, Ordering::Release);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // 仅在首次播放时记录开始时间（play_count 递增在 set_playing 末尾）
        if !self.has_started.swap(true, Ordering::Release) {
            self.start_time_secs.store(now, Ordering::Release);
        }

        self.play_count.fetch_add(1, Ordering::Relaxed);
        info!("播放状态更新: PID={}, 正在播放", pid);
    }

    pub fn set_stopped(&self) {
        self.pid.store(0, Ordering::Release);
        self.running.store(false, Ordering::Release);
        info!("播放状态更新: 已停止");
    }

    pub fn reset_stats(&self) {
        self.play_count.store(0, Ordering::Release);
        self.stop_requested.store(false, Ordering::Release);
        self.next_requested.store(false, Ordering::Release);
        self.has_started.store(false, Ordering::Release);
        self.start_time_secs.store(0, Ordering::Release);
        if let Ok(mut guard) = self.play_msg_id.lock() {
            *guard = None;
        }
        info!("播放统计已重置");
    }

    // ── 状态查询 ──

    pub fn is_playing(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn get_pid(&self) -> u32 {
        self.pid.load(Ordering::Acquire)
    }

    pub fn get_play_count(&self) -> u64 {
        self.play_count.load(Ordering::Acquire)
    }

    pub fn get_start_time(&self) -> Option<u64> {
        if self.has_started.load(Ordering::Acquire) {
            let secs = self.start_time_secs.load(Ordering::Relaxed);
            if secs > 0 {
                return Some(secs);
            }
        }
        None
    }

    pub fn get_play_duration(&self) -> u64 {
        if self.has_started.load(Ordering::Acquire) {
            let start = self.start_time_secs.load(Ordering::Relaxed);
            if start > 0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                return now.saturating_sub(start);
            }
        }
        0
    }

    // ── 请求信号 ──

    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
        self.next_requested.store(false, Ordering::Release);
        info!("请求停止播放");
    }

    pub fn request_next(&self) {
        self.next_requested.store(true, Ordering::Release);
        info!("请求下一首");
    }

    pub fn is_stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
    }

    pub fn is_next_requested(&self) -> bool {
        self.next_requested.load(Ordering::Acquire)
    }

    pub fn clear_next_request(&self) {
        self.next_requested.store(false, Ordering::Release);
    }

    // ── 消息 ID ──

    pub fn set_play_msg_id(&self, msg_id: String) {
        if let Ok(mut guard) = self.play_msg_id.lock() {
            *guard = Some(msg_id);
        }
    }

    pub fn get_play_msg_id(&self) -> Option<String> {
        self.play_msg_id.lock().ok().and_then(|g| g.clone())
    }

    pub fn take_play_msg_id(&self) -> Option<String> {
        self.play_msg_id.lock().ok().and_then(|mut g| g.take())
    }

    // ── 频道 ID ──

    pub fn set_channel_id(&self, channel_id: String) {
        if let Ok(mut guard) = self.channel_id.lock() {
            *guard = Some(channel_id);
        }
    }

    pub fn get_channel_id(&self) -> Option<String> {
        self.channel_id.lock().ok().and_then(|g| g.clone())
    }

    // ── 歌曲时长 ──

    pub fn set_current_song_duration(&self, duration_secs: u64) {
        self.current_song_duration.store(duration_secs, Ordering::Relaxed);
    }

    pub fn get_current_song_duration(&self) -> u64 {
        self.current_song_duration.load(Ordering::Relaxed)
    }

    /// 生成播放进度条字符串
    /// 格式: [████░░] 1:23 / 4:05
    pub fn progress_bar(&self) -> Option<String> {
        let total = self.get_current_song_duration();
        if total == 0 { return None; }
        let elapsed = self.get_play_duration();
        let pct = (elapsed as f64 / total as f64).min(1.0);
        let bar_width = 10;
        let filled = (pct * bar_width as f64) as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);
        Some(format!(
            "[{}] {} / {}",
            bar,
            crate::common::utils::format_duration(elapsed),
            crate::common::utils::format_duration(total)
        ))
    }

    // ── 进程管理 ──

    pub fn kill_process(&self) -> bool {
        let pid = self.pid.load(Ordering::Acquire);
        if pid > 0 {
            #[cfg(target_os = "windows")]
            {
                let output = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
                match output {
                    Ok(o) => {
                        info!("已终止进程 PID={} (Windows)", pid);
                        self.set_stopped();
                        return o.status.success();
                    }
                    Err(e) => {
                        info!("终止进程失败: {}", e);
                        return false;
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                use std::process::Command;
                let output = Command::new("kill").args(["-9", &pid.to_string()]).output();
                match output {
                    Ok(o) => {
                        info!("已终止进程 PID={}", pid);
                        self.set_stopped();
                        return o.status.success();
                    }
                    Err(e) => {
                        info!("终止进程失败: {}", e);
                        return false;
                    }
                }
            }
        }
        false
    }
}
