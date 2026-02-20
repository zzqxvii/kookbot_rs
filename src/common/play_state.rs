use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::RwLock;
use tracing::info;

static PID: AtomicU32 = AtomicU32::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static NEXT_REQUESTED: AtomicBool = AtomicBool::new(false);
static PLAY_MSG_ID: RwLock<Option<String>> = RwLock::new(None);
static CHANNEL_ID: RwLock<Option<String>> = RwLock::new(None);

// 播放统计
static PLAY_COUNT: AtomicU64 = AtomicU64::new(0);
static START_TIME: RwLock<Option<u64>> = RwLock::new(None);

pub fn set_playing(pid: u32) {
    PID.store(pid, Ordering::SeqCst);
    RUNNING.store(true, Ordering::SeqCst);
    STOP_REQUESTED.store(false, Ordering::SeqCst);
    NEXT_REQUESTED.store(false, Ordering::SeqCst);

    // 记录开始时间
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut guard) = START_TIME.write() {
        if guard.is_none() {
            *guard = Some(now);
        }
    }

    // 增加播放计数
    PLAY_COUNT.fetch_add(1, Ordering::SeqCst);

    info!("播放状态更新: PID={}, 正在播放", pid);
}

pub fn set_stopped() {
    PID.store(0, Ordering::SeqCst);
    RUNNING.store(false, Ordering::SeqCst);
    info!("播放状态更新: 已停止");
}

/// 重置所有播放统计（在开始新歌单时调用）
pub fn reset_stats() {
    PLAY_COUNT.store(0, Ordering::SeqCst);
    STOP_REQUESTED.store(false, Ordering::SeqCst);
    NEXT_REQUESTED.store(false, Ordering::SeqCst);
    if let Ok(mut guard) = START_TIME.write() {
        *guard = None;
    }
    if let Ok(mut guard) = PLAY_MSG_ID.write() {
        *guard = None;
    }
    info!("播放统计已重置");
}

/// 获取播放歌曲数量
pub fn get_play_count() -> u64 {
    PLAY_COUNT.load(Ordering::SeqCst)
}

/// 获取开始时间（Unix时间戳）
pub fn get_start_time() -> Option<u64> {
    if let Ok(guard) = START_TIME.read() {
        *guard
    } else {
        None
    }
}

/// 获取播放时长（秒）
pub fn get_play_duration() -> u64 {
    if let Ok(guard) = START_TIME.read() {
        if let Some(start) = *guard {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            return now.saturating_sub(start);
        }
    }
    0
}

pub fn is_playing() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

pub fn get_pid() -> u32 {
    PID.load(Ordering::SeqCst)
}

pub fn request_stop() {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    NEXT_REQUESTED.store(false, Ordering::SeqCst);
    info!("请求停止播放");
}

pub fn request_next() {
    NEXT_REQUESTED.store(true, Ordering::SeqCst);
    info!("请求下一首");
}

pub fn is_stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

pub fn is_next_requested() -> bool {
    NEXT_REQUESTED.load(Ordering::SeqCst)
}

pub fn clear_next_request() {
    NEXT_REQUESTED.store(false, Ordering::SeqCst);
}

pub fn set_play_msg_id(msg_id: String) {
    if let Ok(mut guard) = PLAY_MSG_ID.write() {
        *guard = Some(msg_id);
    }
}

pub fn get_play_msg_id() -> Option<String> {
    if let Ok(guard) = PLAY_MSG_ID.read() {
        guard.clone()
    } else {
        None
    }
}

/// 获取并清除旧的消息ID（用于删除旧卡片）
pub fn take_play_msg_id() -> Option<String> {
    if let Ok(mut guard) = PLAY_MSG_ID.write() {
        guard.take()
    } else {
        None
    }
}

pub fn set_channel_id(channel_id: String) {
    if let Ok(mut guard) = CHANNEL_ID.write() {
        *guard = Some(channel_id);
    }
}

pub fn get_channel_id() -> Option<String> {
    if let Ok(guard) = CHANNEL_ID.read() {
        guard.clone()
    } else {
        None
    }
}

pub fn kill_process() -> bool {
    let pid = PID.load(Ordering::SeqCst);
    if pid > 0 {
        #[cfg(target_os = "windows")]
        {
            let output = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
            match output {
                Ok(o) => {
                    info!("已终止进程 PID={} (Windows)", pid);
                    set_stopped();
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
                    set_stopped();
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
